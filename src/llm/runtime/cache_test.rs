use super::{ProviderCache, ProviderKey};
use crate::config::Config;
use crate::llm::runtime::provider_enum::CachedProvider;
use nu_protocol::LabeledError;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

fn cfg(provider: &str, model: &str) -> Config {
    Config {
        provider: provider.to_string(),
        provider_impl: None,
        model: model.to_string(),
        api_key: None,
        base_url: None,
        temperature: None,
        max_tokens: None,
        max_context_tokens: None,
        max_output_tokens: None,
        max_tool_turns: Some(20),
    }
}

fn test_provider(tag: u64) -> CachedProvider {
    CachedProvider::TestTag(tag)
}

#[test]
fn cache_returns_same_entry_for_same_key() {
    let cache = ProviderCache::new();
    let key = ProviderKey::from_config(&cfg("openai", "gpt-4o"));

    let p1 = cache
        .get_or_create(key.clone(), || Ok(test_provider(1)))
        .expect("first create should succeed");
    let p2 = cache
        .get_or_create(key, || Ok(test_provider(2)))
        .expect("second lookup should succeed");

    assert!(Arc::ptr_eq(&p1, &p2));
}

#[test]
fn cache_miss_creates_new_entry_once() {
    let cache = ProviderCache::new();
    let creates = AtomicU64::new(0);
    let key = ProviderKey::from_config(&cfg("openai", "gpt-4o"));

    let _ = cache
        .get_or_create(key.clone(), || {
            creates.fetch_add(1, Ordering::SeqCst);
            Ok(test_provider(1))
        })
        .expect("create should succeed");

    let _ = cache
        .get_or_create(key, || {
            creates.fetch_add(1, Ordering::SeqCst);
            Ok(test_provider(2))
        })
        .expect("get should succeed");

    assert_eq!(creates.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_get_or_create_is_single_create() {
    let cache = Arc::new(ProviderCache::new());
    let create_count = Arc::new(AtomicU64::new(0));
    let workers = 16;
    let barrier = Arc::new(Barrier::new(workers));

    let mut joins = Vec::new();
    for _ in 0..workers {
        let cache = cache.clone();
        let create_count = create_count.clone();
        let barrier = barrier.clone();
        joins.push(thread::spawn(move || {
            let key = ProviderKey::from_config(&cfg("openai", "gpt-4o"));
            barrier.wait();
            cache.get_or_create(key, || {
                create_count.fetch_add(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(10));
                Ok::<CachedProvider, LabeledError>(test_provider(99))
            })
        }));
    }

    for j in joins {
        let _ = j.join().expect("thread should join").expect("cache op ok");
    }

    assert_eq!(create_count.load(Ordering::SeqCst), 1);
}
