// Closure execution prototype tests
//
// RESEARCH FINDINGS:
//
// EngineInterface DOES support closure execution via eval_closure() method!
// See: https://docs.rs/nu-plugin/latest/nu_plugin/struct.EngineInterface.html
//
// However, closures cannot be created programmatically in tests without
// Nushell's parser. The real use case is:
//   - User passes closure as argument to plugin command
//   - Plugin receives Spanned<Closure> value
//   - Plugin calls engine.eval_closure(&closure, args, input)
//
// APPROACH 1 (PREFERRED): EngineInterface.eval_closure()
// ✅ Works in-process
// ✅ Zero overhead
// ✅ Direct access to engine state
// ✅ Proven in nu-plugin documentation
// ❌ Only available during command execution (not in unit tests)
//
// APPROACH 2 (FALLBACK): MCP via `nu --mcp`
// ✅ Can be tested independently
// ✅ Could work from any context
// ❌ Process overhead (spawn, stdio communication)
// ❌ Requires MCP client implementation
// ❌ More complex error handling
//
// DECISION: Use Approach 1 (EngineInterface.eval_closure)
// - It's the standard way plugins execute closures
// - Well-documented and supported
// - Better performance
// - Simpler implementation
//
// Testing strategy:
// - Create integration test that actually runs the plugin
// - Pass a closure as argument via Nushell
// - Verify the closure executes correctly

use nu_protocol::{BlockId, Span, Spanned, Value, engine::Closure};

/// Mock closure executor trait for testing
///
/// In production, EngineInterface implements this.
/// In tests, we can mock it.
pub trait ClosureExecutor {
    fn eval_closure(
        &self,
        closure: &Spanned<Closure>,
        positional: Vec<Value>,
        input: Option<Value>,
    ) -> Result<Value, String>;
}

/// Wrapper function that executes a closure using any ClosureExecutor
///
/// This demonstrates the actual pattern we'll use in the plugin command.
/// In production, we pass EngineInterface. In tests, we pass a mock.
pub fn execute_tool_closure<E: ClosureExecutor>(
    executor: &E,
    closure: &Spanned<Closure>,
    args: Vec<Value>,
) -> Result<Value, String> {
    executor.eval_closure(closure, args, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock implementation for testing
    struct MockClosureExecutor;

    impl ClosureExecutor for MockClosureExecutor {
        fn eval_closure(
            &self,
            _closure: &Spanned<Closure>,
            positional: Vec<Value>,
            _input: Option<Value>,
        ) -> Result<Value, String> {
            // For testing: simulate {|x| $x + 1} behavior
            if let Some(first_arg) = positional.first()
                && let Ok(n) = first_arg.as_int()
            {
                return Ok(Value::int(n + 1, Span::unknown()));
            }
            Err("Mock: expected integer argument".to_string())
        }
    }

    #[test]
    fn test_closure_executor_trait() {
        // GREEN: This test demonstrates the pattern we'll use
        let executor = MockClosureExecutor;

        // Create a dummy closure (in real usage, this comes from user input)
        let closure = Spanned {
            item: Closure {
                block_id: BlockId::new(0),
                captures: vec![],
            },
            span: Span::unknown(),
        };

        // Execute closure with argument 5
        let result =
            execute_tool_closure(&executor, &closure, vec![Value::int(5, Span::unknown())])
                .expect("Closure execution should succeed");

        // Verify result is 6
        assert_eq!(result.as_int().unwrap(), 6);
    }

    #[test]
    fn test_mock_executor_with_multiple_args() {
        // Test that our abstraction works with multiple arguments
        let executor = MockClosureExecutor;
        let closure = Spanned {
            item: Closure {
                block_id: BlockId::new(0),
                captures: vec![],
            },
            span: Span::unknown(),
        };

        // Our mock only uses first arg, but this shows the pattern
        let result = execute_tool_closure(
            &executor,
            &closure,
            vec![
                Value::int(10, Span::unknown()),
                Value::int(20, Span::unknown()),
            ],
        )
        .expect("Should execute");

        assert_eq!(result.as_int().unwrap(), 11);
    }
}

// PRODUCTION IMPLEMENTATION NOTES:
//
// In the actual agent command, we'll implement ClosureExecutor for EngineInterface:
//
// impl ClosureExecutor for EngineInterface {
//     fn eval_closure(
//         &self,
//         closure: &Spanned<Closure>,
//         positional: Vec<Value>,
//         input: Option<Value>,
//     ) -> Result<Value, String> {
//         self.eval_closure(closure, positional, input)
//             .map_err(|e| e.to_string())
//     }
// }
//
// Then in the command:
//   let tool_result = execute_tool_closure(engine, &tool_closure, args)?;
