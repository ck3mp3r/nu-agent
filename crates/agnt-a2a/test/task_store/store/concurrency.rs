use super::*;

#[test]
fn test_concurrent_writes() -> Result<()> {
    use std::sync::Arc;
    use std::thread;

    let store = Arc::new(InMemoryTaskStore::default());
    let mut handles = vec![];

    for _ in 0..10 {
        let s = store.clone();
        handles.push(thread::spawn(move || {
            s.create_task(None, None, None);
        }));
    }

    for h in handles {
        h.join().map_err(|_| "worker thread panicked")?;
    }

    assert_eq!(
        store.list_tasks(None).len(),
        10,
        "All 10 concurrent creates should succeed"
    );
    Ok(())
}
