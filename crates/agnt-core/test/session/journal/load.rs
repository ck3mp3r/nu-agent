use super::*;

// ---------------------------------------------------------------------------
// Fix 1 tests: repair runs on cache-hit load
// ---------------------------------------------------------------------------

#[tokio::test]
async fn load_cache_hit_runs_repair() {
    let mem = test_memory();

    // Seed the cache directly: `load` then takes the cache-hit path.
    mem.reset_context(
        "conv-1",
        vec![
            Message::user("first"),
            Message::user("second"),
            Message::assistant("reply"),
        ],
    );

    let loaded = mem.load("conv-1").await.unwrap();

    assert_eq!(loaded.len(), 2, "consecutive users should be merged");
    assert!(
        matches!(&loaded[0], Message::User { .. }),
        "first message should be user"
    );
    assert!(
        matches!(&loaded[1], Message::Assistant(_)),
        "second message should be assistant"
    );
}

/// `load` reads the conversation the agent saved in the rig-memory store.
#[tokio::test]
async fn load_reads_the_rig_memory_store() -> Result<()> {
    // -- Setup & Fixtures: a conversation saved through rig's own save path
    let mem = test_memory();
    let store = mem.rig_store("conv-rig")?;
    let mut conversation = rig_memory::load(&*store, "conv-rig")?;
    conversation.extend([Message::user("from rig"), Message::assistant("saved")]);
    rig_memory::save(&*store, "conv-rig", &mut conversation)?;

    // -- Exec
    let loaded = mem.load("conv-rig").await?;

    // -- Check
    assert_eq!(loaded.len(), 2, "the saved conversation is read back");
    assert_msg_eq(&loaded[0], &Message::user("from rig"));
    assert_msg_eq(&loaded[1], &Message::assistant("saved"));
    Ok(())
}

/// `load` returns every stored message in order, and a second load hits the
/// cache instead of re-reading the store.
#[tokio::test]
async fn load_returns_raw_messages_and_is_cached() -> Result<()> {
    // -- Setup & Fixtures
    let mem = test_memory();

    // -- Exec & Check: an empty session loads as empty
    let messages = mem.load("conv-empty").await?;
    assert!(messages.is_empty());

    // -- Exec & Check: messages written to the store are returned
    let msgs = vec![Message::user("hello"), Message::assistant("hi")];
    seed(&mem, "conv-1", &msgs)?;
    let loaded = mem.load("conv-1").await?;
    assert_msgs_eq(&loaded, &msgs);

    // -- Exec & Check: every stored message is returned, in order
    let old = vec![Message::user("old1"), Message::assistant("old2")];
    seed(&mem, "conv-2", &old)?;
    let recent = [Message::user("recent1"), Message::assistant("recent2")];
    append(&mem, "conv-2", &recent)?;

    let loaded = mem.load("conv-2").await?;
    assert_eq!(loaded.len(), 4, "all raw messages are preserved");
    assert_msg_eq(&loaded[0], &old[0]);
    assert_msg_eq(&loaded[1], &old[1]);
    assert_msg_eq(&loaded[2], &recent[0]);
    assert_msg_eq(&loaded[3], &recent[1]);

    // -- Exec & Check: a second load hits the cache instead of re-reading
    let first = mem.load("conv-1").await?;
    assert_eq!(first.len(), 2);
    append(&mem, "conv-1", &[Message::user("extra")])?;
    let second = mem.load("conv-1").await?;
    assert_eq!(
        second.len(),
        2,
        "second load must hit cache, not re-read the store"
    );
    Ok(())
}

// -- Test Support

/// Write `messages` as the conversation of `session_id` in `mem`'s store.
fn seed(mem: &CachedMemory, session_id: &str, messages: &[Message]) -> Result<()> {
    let store = mem.rig_store(session_id)?;
    let mut conversation = rig_memory::load(&*store, session_id)?;
    conversation.extend(messages.iter().cloned());
    rig_memory::save(&*store, session_id, &mut conversation)?;
    Ok(())
}

/// Append `messages` to the conversation of `session_id` in `mem`'s store.
fn append(mem: &CachedMemory, session_id: &str, messages: &[Message]) -> Result<()> {
    seed(mem, session_id, messages)
}
