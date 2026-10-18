//! Retained native namespaces, not host/process migration. Availability and
//! missing-status mutations are labeled simulations; all successful runs are real.
#[path = "support/recovery_continuation_fixture.rs"]
mod fixture;
mod support;
use fixture::*;
use std::{fs, sync::atomic::Ordering, time::Duration};
use workspace_sdk::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_acknowledged_steps_reuse_exact_workspace_with_new_durable_run_claims() {
    run_case("retained-four-steps", |fixture| async move {
        let first = fixture.initial().await;
        let reference = started(&first).workspace;
        let root = fixture.root_of(&first).await;
        let mut run_ids = vec![started(&first).id];
        let mut current = first;
        let mut prior_request: Option<(String, RecoverySession)> = None;
        let mut pending_admission=None;
        let mut samples=Vec::new();
        for epoch in 1..=4 {
            assert_eq!(
                fixture.wait(&current).await,
                RunState::Exited { exit_code: 0 }
            );
            assert_eq!(
                fs::read_to_string(root.join("counter")).unwrap(),
                epoch.to_string()
            );
            let publish_started=std::time::Instant::now();
            let published = fixture
                .recovery
                .publish(&fixture.id, epoch, true)
                .await
                .unwrap();
            let publish_elapsed=publish_started.elapsed();
            assert_eq!(published.committed.generation, epoch);
            if let Some(admission_elapsed)=pending_admission.take() {
                let admission_elapsed:Duration=admission_elapsed;
                samples.push(serde_json::json!({"epoch":epoch,"continue_current_ms":admission_elapsed.as_secs_f64()*1000.0,"publish_ms":publish_elapsed.as_secs_f64()*1000.0}));
            }
            if let Some((name, baseline)) = &prior_request {
                let retry = fixture
                    .reopen()
                    .continue_current(&fixture.id, retained(name, baseline))
                    .await
                    .unwrap();
                same(&started(&retry), &started(&current));
                assert_eq!(
                    fs::read_to_string(root.join("counter")).unwrap(),
                    epoch.to_string(),
                    "published retry must not reexecute"
                );
            }
            if epoch < 4 {
                let name = format!("continue-{epoch}");
                prior_request = Some((name.clone(), published.clone()));
                let admission_started=std::time::Instant::now();
                let accepted = fixture
                    .recovery
                    .continue_current(&fixture.id, retained(&name, &published))
                    .await
                    .unwrap();
                pending_admission=Some(admission_started.elapsed());
                let run = started(&accepted);
                assert_eq!(run.workspace, reference);
                assert!(!run_ids.contains(&run.id));
                run_ids.push(run.id.clone());
                assert_eq!(accepted.current_attempt.as_ref().unwrap().epoch, epoch + 1);
                let reopened = fixture.reopen();
                let retry = reopened
                    .continue_current(&fixture.id, retained(&name, &published))
                    .await
                    .unwrap();
                same(&started(&retry), &run);
                current = retry;
            }
        }
        assert_eq!(fixture.sdk.list().unwrap().len(), 1);
        assert_eq!(fixture.run_count().await, 4);
        assert_eq!(samples.len(),3);
        println!("{}",serde_json::json!({"scenario":"retained_native_small_selected_tree","os":std::env::consts::OS,"arch":std::env::consts::ARCH,"sample_count":samples.len(),"samples":samples,"not_regression_baseline":true}));
        assert_eq!(fixture.root_of(&current).await, root);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retained_launch_before_started_receipt_retry_never_restores_or_executes_twice() {
    for commit_before_error in [false, true] {
        run_case(if commit_before_error{"retained-after-write-loss"}else{"retained-before-write-loss"},move|fixture|async move {
   let head=fixture.acknowledged().await;let root=fixture.root_of(&head).await;
   fixture.faults.commit_before_error.store(commit_before_error,Ordering::Release);fixture.faults.fail.store(true,Ordering::Release);
   assert!(matches!(fixture.recovery.continue_current(&fixture.id,retained("next",&head)).await,Err(Error::State(message)) if message=="injected retained Started receipt failure"));
   let attempted=fixture.faults.attempted.lock().unwrap().clone().unwrap();assert_eq!(fixture.wait(&attempted).await,RunState::Exited{exit_code:0});assert_eq!(fs::read_to_string(root.join("counter")).unwrap(),"2");
   let stable=snapshot(&fixture.session());
   let mut changed=retained("next",&head);
   changed.continuation=CommandSpec::new(support::workload_program()).args(["increment","counter","99"]).into();
   assert!(matches!(fixture.recovery.continue_current(&fixture.id,changed).await,Err(Error::Conflict(_))));
   assert_eq!(snapshot(&fixture.session()),stable);
   // A stable launch retry must not re-capture or restore over later edits.
   fs::write(root.join("after-launch"),"keep user edit").unwrap();
   let retried=fixture.reopen().continue_current(&fixture.id,retained("next",&head)).await.unwrap();same(&started(&retried),&started(&attempted));assert_eq!(fs::read_to_string(root.join("counter")).unwrap(),"2");assert_eq!(fs::read_to_string(root.join("after-launch")).unwrap(),"keep user edit");assert_eq!(fixture.run_count().await,2);assert_eq!(fixture.sdk.list().unwrap().len(),1);
  }).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_head_epoch_quiescence_and_divergence_refuse_without_authority_or_native_mutation() {
    run_case("retained-stale-guards", |fixture| async move {
        let head = fixture.acknowledged().await;
        let root = fixture.root_of(&head).await;
        let before = snapshot(&head);
        for kind in ["epoch", "head", "quiescence"] {
            let mut request = retained(kind, &head);
            match kind {
                "epoch" => request.expected_previous_epoch = 0,
                "head" => request.expected_committed.generation = 0,
                _ => request.quiescent = false,
            };
            assert!(
                matches!(
                    fixture
                        .recovery
                        .continue_current(&fixture.id, request)
                        .await,
                    Err(Error::Conflict(_))
                ),
                "guard {kind}"
            );
            assert_eq!(snapshot(&fixture.session()), before);
            assert_eq!(fixture.run_count().await, 1);
        }
        fs::write(root.join("counter"), "user divergence").unwrap();
        assert!(matches!(
            fixture
                .recovery
                .continue_current(&fixture.id, retained("divergent", &head))
                .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(&fixture.session()), before);
        assert_eq!(fixture.run_count().await, 1);
        assert_eq!(
            fs::read_to_string(root.join("counter")).unwrap(),
            "user divergence"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unacknowledged_active_or_completed_native_owner_cannot_continue() {
    run_case("retained-unacknowledged", |fixture| async move {
        let mut request = fresh("initial", 0, "laptop");
        request.continuation = CommandSpec::new(support::workload_program())
            .args(["sleep", "2"])
            .into();
        let active = fixture
            .recovery
            .recover(&fixture.id, request)
            .await
            .unwrap();
        let before = snapshot(&active);
        let reference = started(&active);
        assert!(fixture
            .sdk
            .connect(&reference.workspace)
            .await
            .unwrap()
            .run(reference.id)
            .state()
            .await
            .unwrap()
            .is_active());
        assert!(matches!(
            fixture
                .recovery
                .continue_current(&fixture.id, retained("active", &active))
                .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(&fixture.session()), before);
        assert_eq!(
            fixture.wait(&active).await,
            RunState::Exited { exit_code: 0 }
        );
        assert!(matches!(
            fixture
                .recovery
                .continue_current(&fixture.id, retained("unpublished", &active))
                .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(&fixture.session()), before);
        assert_eq!(fixture.run_count().await, 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn published_nonzero_canceled_and_timed_out_native_owners_are_not_successful_continuations() {
    for state in ["nonzero", "canceled", "timedout"] {
        run_case(&format!("retained-{state}"), move |fixture| async move {
            let mut request = fresh("initial", 0, "laptop");
            request.continuation = match state {
                "nonzero" => CommandSpec::new("/bin/sh").args(["-c", "exit 7"]).into(),
                "canceled" => CommandSpec::new(support::workload_program())
                    .args(["sleep", "30"])
                    .into(),
                _ => CommandSpec::new(support::workload_program())
                    .args(["sleep", "30"])
                    .timeout(0)
                    .into(),
            };
            let initial = fixture
                .recovery
                .recover(&fixture.id, request)
                .await
                .unwrap();
            if state == "canceled" {
                let run = started(&initial);
                fixture
                    .sdk
                    .connect(&run.workspace)
                    .await
                    .unwrap()
                    .run(run.id)
                    .cancel()
                    .await
                    .unwrap();
            }
            let terminal = fixture.wait(&initial).await;
            assert_eq!(
                terminal,
                match state {
                    "nonzero" => RunState::Exited { exit_code: 7 },
                    "canceled" => RunState::Canceled,
                    _ => RunState::TimedOut,
                }
            );
            let head = fixture
                .recovery
                .publish(&fixture.id, 1, true)
                .await
                .unwrap();
            let before = snapshot(&head);
            assert!(matches!(
                fixture
                    .recovery
                    .continue_current(&fixture.id, retained("refused", &head))
                    .await,
                Err(Error::Conflict(_))
            ));
            assert_eq!(snapshot(&fixture.session()), before);
            assert_eq!(fixture.run_count().await, 1);
        })
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn simulated_unreachable_and_lost_authority_never_authorize_a_replacement() {
    run_case("retained-unreachable-lost", |fixture| async move {
        let head = fixture.acknowledged().await;
        let before = snapshot(&head);
        fixture.online.store(false, Ordering::Release);
        assert!(matches!(
            fixture
                .recovery
                .continue_current(&fixture.id, retained("offline", &head))
                .await,
            Err(Error::Provider { provider,message }) if provider=="native-fixture" && message=="modeled unavailable transport"
        ));
        fixture.online.store(true, Ordering::Release);
        assert_eq!(snapshot(&fixture.session()), before);
        assert_eq!(fixture.run_count().await, 1);
        // Deliberately mutate the closed fixture receipt to model missing ownership;
        // this is not evidence of physical host loss. Restore it before cleanup.
        let run = started(&head);
        let descriptor = fixture
            .sdk
            .connect(&run.workspace)
            .await
            .unwrap()
            .descriptor();
        let status = std::path::Path::new(&descriptor.control_root)
            .join("runs")
            .join(run.id.as_str())
            .join("status.json");
        fixture.owner_released(&head).await;
        let original = fs::read(&status).unwrap();
        fs::write(
            &status,
            serde_json::to_vec(&RunState::Lost {
                reason: "modeled ownership loss".into(),
            })
            .unwrap(),
        )
        .unwrap();
        let refused = fixture
            .recovery
            .continue_current(&fixture.id, retained("lost", &head))
            .await;
        fs::write(&status, original).unwrap();
        assert!(matches!(refused, Err(Error::Conflict(_))));
        assert_eq!(snapshot(&fixture.session()), before);
        assert_eq!(fixture.run_count().await, 1);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_old_workspace_after_fresh_switch_cannot_be_selected_for_retained_work() {
    run_case("retained-stale-workspace", |fixture| async move {
        let old = fixture.acknowledged().await;
        let old_root = fixture.root_of(&old).await;
        let switched = fixture
            .recovery
            .recover(&fixture.id, fresh("fresh-switch", 1, "devbox"))
            .await
            .unwrap();
        assert_eq!(
            fixture.wait(&switched).await,
            RunState::Exited { exit_code: 0 }
        );
        let head = fixture
            .recovery
            .publish(&fixture.id, 2, true)
            .await
            .unwrap();
        let before = snapshot(&head);
        let target = started(&head).workspace;
        let target_root = fixture.root_of(&head).await;
        fs::write(old_root.join("counter"), "old laptop edit").unwrap();
        assert!(matches!(
            fixture
                .recovery
                .continue_current(&fixture.id, retained("old-laptop", &old))
                .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(&fixture.session()), before);
        assert_eq!(fixture.run_count().await, 2);
        assert_eq!(
            fs::read_to_string(old_root.join("counter")).unwrap(),
            "old laptop edit"
        );
        let next = fixture
            .recovery
            .continue_current(&fixture.id, retained("current-devbox", &head))
            .await
            .unwrap();
        assert_eq!(started(&next).workspace, target);
        assert_eq!(fixture.wait(&next).await, RunState::Exited { exit_code: 0 });
        assert_eq!(
            fs::read_to_string(target_root.join("counter")).unwrap(),
            "3"
        );
        assert_eq!(
            fs::read_to_string(old_root.join("counter")).unwrap(),
            "old laptop edit"
        );
        assert_eq!(fixture.sdk.list().unwrap().len(), 2);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retained_and_fresh_entrypoints_cannot_bypass_each_others_preconditions() {
    run_case("retained-entrypoint-guards", |fixture| async move {
        let head = fixture.acknowledged().await;
        let before = snapshot(&head);
        let mut wrong = retained("initial", &head);
        wrong.expected_previous_epoch = 0;
        assert!(matches!(
            fixture.recovery.continue_current(&fixture.id, wrong).await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(&fixture.session()), before);
        let accepted = fixture
            .recovery
            .continue_current(&fixture.id, retained("retained", &head))
            .await
            .unwrap();
        assert_eq!(
            fixture.wait(&accepted).await,
            RunState::Exited { exit_code: 0 }
        );
        let stable = snapshot(&accepted);
        assert!(matches!(
            fixture
                .recovery
                .recover(&fixture.id, fresh("retained", 1, "laptop"))
                .await,
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(&fixture.session()), stable);
        assert_eq!(fixture.run_count().await, 2);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn competing_retained_requests_and_fresh_recovery_share_one_effect_authority() {
    for race in ["same", "different", "fresh"] {
        run_case(
            &format!("retained-race-{race}"),
            move |fixture| async move {
                let head = fixture.acknowledged().await;
                let left = fixture
                    .recovery
                    .continue_current(&fixture.id, retained("left", &head));
                let right = async {
                    if race == "fresh" {
                        fixture
                            .recovery
                            .recover(&fixture.id, fresh("fresh", 1, "devbox"))
                            .await
                    } else {
                        fixture
                            .recovery
                            .continue_current(
                                &fixture.id,
                                retained(if race == "same" { "left" } else { "right" }, &head),
                            )
                            .await
                    }
                };
                let (left, right) = tokio::join!(left, right);
                assert!(
                    left.is_ok() || right.is_ok(),
                    "one authority must admit work"
                );
                let accepted = left.as_ref().ok().or(right.as_ref().ok()).unwrap();
                for result in [&left, &right] {
                    match result {
                        Ok(other) => same(&started(other), &started(accepted)),
                        Err(error) => assert!(
                            matches!(error, Error::Conflict(_)),
                            "unexpected race result {error:?}"
                        ),
                    }
                }
                if race != "same" {
                    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
                }
                assert_eq!(
                    fixture.wait(accepted).await,
                    RunState::Exited { exit_code: 0 }
                );
                assert_eq!(fixture.run_count().await, 2);
                assert_eq!(fixture.session().current_attempt.unwrap().epoch, 2);
                let root = fixture.root_of(accepted).await;
                assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "2");
                assert_eq!(
                    fixture.sdk.list().unwrap().len(),
                    if started(accepted).workspace == started(&head).workspace {
                        1
                    } else {
                        2
                    }
                );
            },
        )
        .await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn caller_abort_after_native_start_keeps_effect_lease_until_started_receipt_finishes() {
    run_case("retained-caller-abort", |fixture| async move {
        let head = fixture.acknowledged().await;
        let (entered, release) = fixture.faults.gate();
        let task = tokio::spawn({
            let fixture = fixture.clone();
            let head = head.clone();
            async move {
                fixture
                    .recovery
                    .continue_current(&fixture.id, retained("owned", &head))
                    .await
            }
        });
        tokio::task::spawn_blocking(move || entered.recv_timeout(Duration::from_secs(10)))
            .await
            .unwrap()
            .expect("real Started receipt gate not reached");
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let competing = fixture
            .recovery
            .continue_current(&fixture.id, retained("competing", &head))
            .await;
        release.send(()).unwrap();
        assert!(
            matches!(competing, Err(Error::Conflict(_))),
            "caller cancellation released the native effect authority"
        );
        let accepted = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let current = fixture.session();
                if current.current_attempt.as_ref().is_some_and(|attempt| {
                    attempt.epoch == 2 && matches!(attempt.phase, RecoveryPhase::Started { .. })
                }) {
                    break current;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            fixture.wait(&accepted).await,
            RunState::Exited { exit_code: 0 }
        );
        let retry = fixture
            .reopen()
            .continue_current(&fixture.id, retained("owned", &head))
            .await
            .unwrap();
        same(&started(&retry), &started(&accepted));
        assert_eq!(fixture.run_count().await, 2);
        let root = fixture.root_of(&accepted).await;
        assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "2");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prelaunch_preparing_and_restored_retries_recheck_selected_tree_before_native_start() {
    for commit_before_error in [false, true] {
        let name = if commit_before_error {
            "retained-prelaunch-restored"
        } else {
            "retained-prelaunch-preparing"
        };
        run_case(name, move |fixture| async move {
            let head = fixture.acknowledged().await;
            let root = fixture.root_of(&head).await;
            fixture.faults.fail_restored.store(true, Ordering::Release);
            fixture.faults.commit_before_error.store(commit_before_error, Ordering::Release);
            fixture.faults.fail.store(true, Ordering::Release);
            assert!(matches!(
                fixture.recovery.continue_current(&fixture.id, retained("prelaunch", &head)).await,
                Err(Error::State(message)) if message == "injected retained Restored receipt failure"
            ));
            let pending = fixture.session();
            let intent = pending.current_attempt.as_ref().unwrap().clone();
            if commit_before_error {
                assert!(matches!(intent.phase, RecoveryPhase::Restored { .. }));
            } else {
                assert!(matches!(intent.phase, RecoveryPhase::Preparing));
            }
            assert_eq!(pending.committed, head.committed);
            assert_eq!(fixture.run_count().await, 1, "no successor native claim exists before launch intent");
            let mut changed = retained("prelaunch", &head);
            changed.continuation = CommandSpec::new(support::workload_program())
                .args(["increment", "counter", "99"]).into();
            assert!(matches!(fixture.recovery.continue_current(&fixture.id, changed).await, Err(Error::Conflict(_))));
            assert_eq!(fixture.run_count().await, 1);
            assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "1");
            // Initial verification succeeded before the injected CAS failure.
            // A new prelaunch call must establish it again, not reuse that proof.
            fs::write(root.join("counter"), "selected edit after saved intent").unwrap();
            assert!(matches!(
                fixture.reopen().continue_current(&fixture.id, retained("prelaunch", &head)).await,
                Err(Error::Conflict(_))
            ));
            let refused = fixture.session();
            let after = refused.current_attempt.as_ref().unwrap();
            assert_eq!(refused.committed, head.committed);
            assert_eq!(after.request_id, intent.request_id);
            assert_eq!(after.expected_previous_epoch, intent.expected_previous_epoch);
            assert_eq!(after.epoch, intent.epoch);
            assert_eq!(after.workspace_id, intent.workspace_id);
            assert_eq!(after.run_id, intent.run_id);
            assert_eq!(after.checkpoint, intent.checkpoint);
            assert_eq!(serde_json::to_value(&after.retained).unwrap(), serde_json::to_value(&intent.retained).unwrap());
            assert!(matches!(after.phase, RecoveryPhase::Preparing | RecoveryPhase::Restored { .. }), "refused recheck must not publish launch intent");
            assert_eq!(fixture.run_count().await, 1);
            assert_eq!(fixture.sdk.list().unwrap().len(), 1);
            assert_eq!(fs::read_to_string(root.join("counter")).unwrap(), "selected edit after saved intent");
        }).await;
    }
}
