use super::*;
use async_trait::async_trait;
use std::sync::Mutex;

/// A cloud in memory: its targets by key, and what its next request does.
struct Site {
    idempotency: Idempotency,
    targets: Mutex<Vec<(String, u32)>>,
    /// What each submit does, in turn.
    submits: Mutex<Vec<Submit>>,
    finds_fail_after: Option<usize>,
    calls: Mutex<Vec<&'static str>>,
}
#[derive(Clone, Copy)]
enum Submit {
    /// Makes a target and names it.
    Answer,
    /// Makes a target and loses the answer.
    LoseAnswer,
    /// Makes nothing and loses the answer.
    Fail,
    /// Refuses for certain.
    Refuse,
}
impl Site {
    fn new(idempotency: Idempotency, submits: &[Submit]) -> Self {
        Self {
            idempotency,
            targets: Mutex::new(Vec::new()),
            submits: Mutex::new(submits.iter().rev().copied().collect()),
            finds_fail_after: None,
            calls: Mutex::new(Vec::new()),
        }
    }
    fn with(self, key: &str, count: u32) -> Self {
        for id in 0..count {
            self.targets.lock().unwrap().push((key.into(), 100 + id));
        }
        self
    }
    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }
}
#[async_trait]
impl ICreationSite for Site {
    type Target = u32;
    type Error = &'static str;
    fn idempotency(&self) -> Idempotency {
        self.idempotency
    }
    async fn find(&self, key: &str) -> Result<Vec<u32>, &'static str> {
        let mut calls = self.calls.lock().unwrap();
        calls.push("find");
        if self
            .finds_fail_after
            .is_some_and(|after| calls.iter().filter(|c| **c == "find").count() > after)
        {
            return Err("find failed");
        }
        Ok(self
            .targets
            .lock()
            .unwrap()
            .iter()
            .filter(|(tag, _)| tag == key)
            .map(|(_, id)| *id)
            .collect())
    }
    async fn submit(&self, key: &str) -> Result<Submitted<u32>, &'static str> {
        self.calls.lock().unwrap().push("submit");
        let submit = self
            .submits
            .lock()
            .unwrap()
            .pop()
            .expect("an unexpected submit");
        let mut targets = self.targets.lock().unwrap();
        let id = targets.len() as u32 + 1;
        match submit {
            Submit::Answer => {
                targets.push((key.into(), id));
                Ok(Submitted::Named(id))
            }
            Submit::LoseAnswer => {
                targets.push((key.into(), id));
                Ok(Submitted::Unknown)
            }
            Submit::Fail => Ok(Submitted::Unknown),
            Submit::Refuse => Err("refused"),
        }
    }
}

#[tokio::test]
async fn a_tagged_cloud_finds_an_earlier_attempts_target_before_asking() {
    let site = Site::new(Idempotency::Tagged, &[]).with("k", 1);
    assert_eq!(
        find_or_create(&site, "k").await,
        Ok((100, CreationOrigin::FoundBefore))
    );
    assert_eq!(site.calls(), ["find"]);
}

#[tokio::test]
async fn a_tagged_cloud_makes_one_when_none_carries_the_key() {
    let site = Site::new(Idempotency::Tagged, &[Submit::Answer]).with("other", 1);
    assert_eq!(
        find_or_create(&site, "k").await,
        Ok((2, CreationOrigin::Answered))
    );
    // Asked again with the key, the same one is found, and no second made.
    assert_eq!(
        find_or_create(&site, "k").await,
        Ok((2, CreationOrigin::FoundBefore))
    );
    assert_eq!(site.calls(), ["find", "submit", "find"]);
}

#[tokio::test]
async fn a_lost_answer_is_settled_by_finding_what_it_made() {
    let site = Site::new(Idempotency::Tagged, &[Submit::LoseAnswer]);
    assert_eq!(
        find_or_create(&site, "k").await,
        Ok((1, CreationOrigin::FoundAfterLostAnswer))
    );
    assert_eq!(site.calls(), ["find", "submit", "find"]);
}

#[tokio::test]
async fn a_lost_answer_that_made_nothing_findable_is_uncertain_and_never_resent() {
    let site = Site::new(Idempotency::Tagged, &[Submit::Fail]);
    assert_eq!(
        find_or_create(&site, "k").await,
        Err(CreationFailure::Uncertain)
    );
    // A find that fails after a lost answer is uncertain too, not a refusal.
    let mut site = Site::new(Idempotency::Tagged, &[Submit::LoseAnswer]);
    site.finds_fail_after = Some(1);
    assert_eq!(
        find_or_create(&site, "k").await,
        Err(CreationFailure::Uncertain)
    );
    assert_eq!(site.calls(), ["find", "submit", "find"]);
}

#[tokio::test]
async fn two_targets_carrying_one_key_are_reported_never_chosen() {
    let site = Site::new(Idempotency::Tagged, &[]).with("k", 2);
    assert_eq!(
        find_or_create(&site, "k").await,
        Err(CreationFailure::Ambiguous)
    );
    // Also when a lost answer turns out to have made a second.
    let site = Site::new(Idempotency::Tagged, &[Submit::LoseAnswer]);
    site.targets.lock().unwrap().push(("k".into(), 7));
    let site = Site {
        finds_fail_after: None,
        ..site
    };
    // The first find sees one already: it is found, not duplicated.
    assert_eq!(
        find_or_create(&site, "k").await,
        Ok((7, CreationOrigin::FoundBefore))
    );
}

#[tokio::test]
async fn a_find_that_fails_before_asking_is_the_clouds_failure() {
    let mut site = Site::new(Idempotency::Tagged, &[]);
    site.finds_fail_after = Some(0);
    assert_eq!(
        find_or_create(&site, "k").await,
        Err(CreationFailure::Site("find failed"))
    );
    assert_eq!(site.calls(), ["find"]);
}

#[tokio::test]
async fn a_native_cloud_is_only_asked_and_a_lost_answer_is_uncertain() {
    let site = Site::new(Idempotency::Native, &[Submit::Answer, Submit::LoseAnswer]);
    assert_eq!(
        find_or_create(&site, "k").await,
        Ok((1, CreationOrigin::Answered))
    );
    assert_eq!(
        find_or_create(&site, "k").await,
        Err(CreationFailure::Uncertain)
    );
    assert_eq!(site.calls(), ["submit", "submit"]);
}

#[tokio::test]
async fn a_certain_refusal_is_the_clouds_failure() {
    for idempotency in [Idempotency::Native, Idempotency::Tagged] {
        let site = Site::new(idempotency, &[Submit::Refuse]);
        assert_eq!(
            find_or_create(&site, "k").await,
            Err(CreationFailure::Site("refused"))
        );
    }
}
