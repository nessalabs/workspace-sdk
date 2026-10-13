use super::*;

struct Interaction {
    service: PreparationService,
    host: Arc<FakeHost>,
    store: Arc<MemoryPreparationStore>,
    request: PreparationRequest,
    action: AuthenticationAction,
    process: RuntimeProcessRef,
}

async fn interaction() -> Interaction {
    let (service, host, _, store) = service();
    let request = request(claude_code("2.1.0"), Authentication::Subscription);
    let PreparationOutcome::NeedsAuthentication { action, .. } =
        service.prepare(&request).await.unwrap()
    else {
        panic!("expected native login action")
    };
    let process = service
        .start_authentication(&request, &action)
        .await
        .unwrap()
        .process
        .unwrap();
    Interaction {
        service,
        host,
        store,
        request,
        action,
        process,
    }
}

#[tokio::test]
async fn controls_preserve_attempt_identity_and_do_not_persist_echoed_input() {
    let flow = interaction().await;
    let input = SecretInput::new(b"echoed-private-input\r".to_vec());
    assert_eq!(
        flow.service
            .write_authentication_input(&flow.request, &flow.process, &input)
            .await
            .unwrap(),
        RuntimeInputOutcome::Accepted {
            bytes: input.expose().len()
        }
    );
    let size = RuntimeTerminalSize::new(120, 40).unwrap();
    flow.service
        .resize_authentication(&flow.request, &flow.process, size)
        .await
        .unwrap();
    let page = flow
        .service
        .read_authentication_output(&flow.request, &flow.process, OutputCursor::default(), 128)
        .await
        .unwrap();
    assert_eq!(page.stdout, "echoed-private-input");
    assert!(!format!("{page:?} {input:?}").contains("echoed-private-input"));
    assert_eq!(flow.host.0.lock().unwrap().resizes, vec![size]);
    let receipt = flow.store.load(&flow.request.id).await.unwrap().unwrap();
    assert!(!serde_json::to_string(&receipt)
        .unwrap()
        .contains("echoed-private-input"));
    assert_eq!(
        flow.service
            .cancel_authentication(&flow.request, &flow.process)
            .await
            .unwrap(),
        RuntimeProcessState::Exited { code: 130 }
    );
    assert!(matches!(
        flow.service
            .check_authentication(&flow.request)
            .await
            .unwrap(),
        AuthenticationState::NeedsAuthentication(_)
    ));
    let next = flow
        .service
        .start_authentication(&flow.request, &flow.action)
        .await
        .unwrap()
        .process
        .unwrap();
    assert_ne!(next, flow.process);
    assert!(matches!(
        flow.service
            .write_authentication_input(&flow.request, &flow.process, &input)
            .await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(flow.host.0.lock().unwrap().inputs.len(), 1);
}

#[tokio::test]
async fn partial_and_unknown_input_are_returned_without_automatic_replay() {
    let flow = interaction().await;
    let input = SecretInput::new("code-λ\r".as_bytes().to_vec());
    for expected in [
        RuntimeInputOutcome::Accepted { bytes: 3 },
        RuntimeInputOutcome::Unknown,
    ] {
        flow.host.0.lock().unwrap().input_outcome = Some(expected);
        assert_eq!(
            flow.service
                .write_authentication_input(&flow.request, &flow.process, &input)
                .await
                .unwrap(),
            expected
        );
    }
    flow.host.0.lock().unwrap().input_outcome =
        Some(RuntimeInputOutcome::Accepted { bytes: 100_000 });
    assert_eq!(
        flow.service
            .write_authentication_input(&flow.request, &flow.process, &input)
            .await
            .unwrap(),
        RuntimeInputOutcome::Unknown
    );
    assert_eq!(flow.host.0.lock().unwrap().inputs.len(), 3);
}

#[tokio::test]
async fn timed_out_input_acknowledgement_does_not_clear_or_repeat_the_attempt() {
    let flow = interaction().await;
    flow.host.0.lock().unwrap().stall_input = true;
    let result = flow
        .service
        .write_authentication_input(
            &flow.request,
            &flow.process,
            &SecretInput::new(b"private-code\r".to_vec()),
        )
        .await
        .unwrap();
    assert_eq!(result, RuntimeInputOutcome::Unknown);
    assert_eq!(flow.host.0.lock().unwrap().inputs.len(), 1);
    assert_eq!(
        flow.store
            .load(&flow.request.id)
            .await
            .unwrap()
            .unwrap()
            .authentication_process,
        Some(flow.process)
    );
}

#[tokio::test]
async fn input_output_and_size_bounds_are_enforced_before_delivery() {
    let flow = interaction().await;
    for input in [Vec::new(), vec![0; 65_537]] {
        assert!(matches!(
            flow.service
                .write_authentication_input(&flow.request, &flow.process, &SecretInput::new(input))
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    assert!(flow.host.0.lock().unwrap().inputs.is_empty());
    for limit in [0, 1, 131_073] {
        assert!(matches!(
            flow.service
                .read_authentication_output(
                    &flow.request,
                    &flow.process,
                    OutputCursor::default(),
                    limit
                )
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    flow.host.0.lock().unwrap().oversized_output = true;
    assert!(matches!(
        flow.service
            .read_authentication_output(&flow.request, &flow.process, OutputCursor::default(), 64)
            .await,
        Err(Error::State(_))
    ));
    assert!(RuntimeTerminalSize::new(0, 24).is_err());
    assert!(RuntimeTerminalSize::new(1_000, 1_000).is_err());
}

#[tokio::test]
async fn wrong_target_foreign_scope_and_forged_process_fail_before_host_calls() {
    let flow = interaction().await;
    let calls = flow.host.0.lock().unwrap().commands.len();
    let mut foreign = flow.request.clone();
    foreign.target_id = "workspace-other".into();
    assert!(matches!(
        flow.service.prepare(&foreign).await,
        Err(Error::InvalidInput(_))
    ));
    for process in [
        RuntimeProcessRef::new("another-host", flow.process.id()).unwrap(),
        RuntimeProcessRef::new(flow.process.host_scope(), "different-login").unwrap(),
    ] {
        assert!(matches!(
            flow.service
                .cancel_authentication(&flow.request, &process)
                .await,
            Err(Error::Conflict(_))
        ));
    }
    assert_eq!(flow.host.0.lock().unwrap().commands.len(), calls);
    assert!(!flow.host.0.lock().unwrap().process_exited);
    assert!(serde_json::from_str::<RuntimeProcessRef>(r#"{"id":"legacy-unscoped"}"#).is_err());
}

#[tokio::test]
async fn cancellation_with_lost_acknowledgement_retains_the_original_receipt() {
    let flow = interaction().await;
    flow.host.0.lock().unwrap().cancel_loses_response = true;
    assert!(flow
        .service
        .cancel_authentication(&flow.request, &flow.process)
        .await
        .is_err());
    assert_eq!(
        flow.store
            .load(&flow.request.id)
            .await
            .unwrap()
            .unwrap()
            .authentication_process,
        Some(flow.process.clone())
    );
    assert_eq!(
        flow.service
            .authentication_process_state(&flow.request, &flow.process)
            .await
            .unwrap(),
        RuntimeProcessState::Exited { code: 1 }
    );
    assert_eq!(flow.host.0.lock().unwrap().login_calls, 1);
}

#[tokio::test]
async fn lost_state_never_allows_input_or_replacement_without_terminal_evidence() {
    let flow = interaction().await;
    flow.host.0.lock().unwrap().process_lost = true;
    assert_eq!(
        flow.service
            .authentication_process_state(&flow.request, &flow.process)
            .await
            .unwrap(),
        RuntimeProcessState::Lost
    );
    assert!(flow
        .service
        .start_authentication(&flow.request, &flow.action)
        .await
        .is_err());
    assert!(flow
        .service
        .write_authentication_input(
            &flow.request,
            &flow.process,
            &SecretInput::new(b"code\r".to_vec())
        )
        .await
        .is_err());
    assert_eq!(flow.host.0.lock().unwrap().login_calls, 1);
    assert!(flow.host.0.lock().unwrap().inputs.is_empty());
    flow.host.0.lock().unwrap().process_lost = false;
    assert_eq!(
        flow.service
            .authentication_process_state(&flow.request, &flow.process)
            .await
            .unwrap(),
        RuntimeProcessState::Running
    );
}
