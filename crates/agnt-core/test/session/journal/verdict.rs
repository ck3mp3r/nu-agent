use super::support::{tool_result, tool_result_flag, tool_result_flags, tool_result_message};
use super::*;

/// Tool-verdict stamping: a recorded verdict is stamped as
/// `ToolResult::is_error` onto the matching persisted ToolResult, consumed on
/// first stamp, and applied per call id within a batched append.
#[tokio::test]
async fn append_stamps_recorded_tool_verdicts() -> Result<()> {
    // -- Setup & Fixtures
    // (name, recorded verdicts, append batches, expected flags per entry)
    struct Case {
        name: &'static str,
        verdicts: &'static [(&'static str, bool)],
        batches: Vec<Vec<Message>>,
        expected: Vec<Vec<Option<bool>>>,
    }

    let cases = vec![
        Case {
            name: "recorded true verdict",
            verdicts: &[("tc1", true)],
            batches: vec![vec![tool_result_message("tc1", "all good")?]],
            expected: vec![vec![Some(true)]],
        },
        Case {
            name: "recorded false verdict",
            verdicts: &[("tc1", false)],
            batches: vec![vec![tool_result_message("tc1", "the tool failed")?]],
            expected: vec![vec![Some(false)]],
        },
        Case {
            name: "verdict is consumed after the first stamp",
            verdicts: &[("tc1", false)],
            batches: vec![
                vec![tool_result_message("tc1", "first")?],
                vec![tool_result_message("tc1", "second")?],
            ],
            expected: vec![vec![Some(false)], vec![Some(true)]],
        },
        Case {
            name: "no recorded verdict keeps the default success state",
            verdicts: &[],
            batches: vec![vec![tool_result_message("tc1", "plain output")?]],
            expected: vec![vec![Some(true)]],
        },
        Case {
            name: "batched tool results are stamped per call id",
            verdicts: &[("tc1", true), ("tc2", false)],
            batches: vec![vec![Message::User {
                content: vec![
                    tool_result("tc1", "first ok")?,
                    tool_result("tc2", "second failed")?,
                ],
            }]],
            expected: vec![vec![Some(true), Some(false)]],
        },
    ];

    // -- Exec & Check
    for case in cases {
        let tmp = TempDir::new().map_err(|e| format!("tempdir: {e:?}"))?;
        let mem = TestMemory::new(Arc::new(FsSessionStore::new(tmp.path().to_path_buf())));
        for (call_id, verdict) in case.verdicts {
            mem.record_tool_verdict(call_id, *verdict);
        }

        let conv_id = format!("conv-{}", case.name.replace(' ', "-"));
        for batch in case.batches {
            mem.append(&ConversationId::from(conv_id.as_str()), batch)
                .await
                .map_err(|e| format!("{}: append: {e:?}", case.name))?;
        }

        let entries = mem
            .load_all(&conv_id)
            .await
            .map_err(|e| format!("{}: load_all: {e:?}", case.name))?;
        assert_eq!(entries.len(), case.expected.len(), "{}", case.name);
        for (entry, expected) in entries.iter().zip(case.expected.iter()) {
            assert_eq!(&tool_result_flags(entry), expected, "{}", case.name);
        }
        // The single-result helper must agree with the batch helper.
        if let (Some(entry), Some(expected)) = (entries.first(), case.expected.first())
            && expected.len() == 1
        {
            assert_eq!(tool_result_flag(entry), expected[0], "{}", case.name);
        }
    }
    Ok(())
}
