use chaffnet_core::classifier::BaselineClassifier;
use chaffnet_core::config::EngineConfig;
use chaffnet_core::content::{Content, ContentContext};
use chaffnet_core::reputation::MemoryStore;
use chaffnet_core::Engine;
use std::net::{IpAddr, Ipv4Addr};

#[test]
fn reputation_pushes_borderline_content_over_the_line() {
    // A short, link-free message that rules alone would pass.
    let text = "hey check this out its pretty good you should look";
    let clean_engine = Engine::new(
        MemoryStore::new(),
        BaselineClassifier::default(),
        EngineConfig::default(),
    );
    let mut c = Content::new(text, ContentContext::Comment);
    c.author_ip = Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));
    let baseline = clean_engine.assess(&c).unwrap();

    // Same content, but the IP bucket is a known spammer.
    let mut store = MemoryStore::new();
    let bucket = chaffnet_core::fingerprint::ip_bucket(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));
    store.set_ip_bucket_score(bucket, 0.95);
    let rep_engine = Engine::new(
        store,
        BaselineClassifier::default(),
        EngineConfig::default(),
    );
    let with_rep = rep_engine.assess(&c).unwrap();

    assert!(with_rep.spam > baseline.spam);
    assert!(with_rep
        .reasons
        .contains(&chaffnet_core::ReasonCode::IpReputationBad));
}
