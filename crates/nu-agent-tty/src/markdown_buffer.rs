/// Buffers streamed markdown text and only yields the safe prefix where all
/// open constructs (code fences, inline code, bold, italic, links) are closed.
///
/// Streaming LLM responses deliver the accumulated text in chunks. A chunk may
/// end mid-construct (e.g. `**bold` with no closing `**`), which would render
/// literal markers in the terminal until the close arrives later. This buffer
/// holds partial constructs and releases text only once a clean boundary is
/// reached, so the terminal never shows broken markdown.
#[derive(Default)]
pub struct StreamingMarkdownBuffer {
    accumulated: String,
    last_flushed_len: usize,
}

impl StreamingMarkdownBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a new text delta and return the newly-safe prefix to render.
    ///
    /// Rescans the entire accumulated buffer from the start and returns the
    /// longest leading run (starting at `last_flushed_len`) that ends at a clean
    /// boundary where no markdown construct is open. Returns an empty string when
    /// the tail is still inside an open construct.
    pub fn push(&mut self, delta: &str) -> String {
        self.accumulated.push_str(delta);
        let safe_end = self.last_clean_boundary();
        if safe_end > self.last_flushed_len {
            let out = self.accumulated[self.last_flushed_len..safe_end].to_string();
            self.last_flushed_len = safe_end;
            out
        } else {
            String::new()
        }
    }

    /// Return all remaining buffered text, including any unclosed constructs.
    pub fn flush(&mut self) -> String {
        let rest = self.accumulated[self.last_flushed_len..].to_string();
        self.last_flushed_len = self.accumulated.len();
        rest
    }

    /// Clear all state for a new turn.
    pub fn reset(&mut self) {
        self.accumulated.clear();
        self.last_flushed_len = 0;
    }

    /// Scan `accumulated` from byte 0 and return the byte index (exclusive) of
    /// the last clean boundary — the position after which no markdown construct
    /// is open. Returns 0 when the whole buffer is inside a construct.
    fn last_clean_boundary(&self) -> usize {
        let bytes = self.accumulated.as_bytes();
        let n = bytes.len();
        let mut i = 0;
        let mut last_clean = 0;
        let mut in_code_fence = false;
        let mut fence_char = '\0';
        let mut fence_len = 0;
        let mut in_inline_code = false;
        let mut in_bold = false;
        let mut in_italic = false;
        let mut in_link = false;

        while i < n {
            let c = bytes[i] as char;
            match c {
                '`' => {
                    let run = count_run(bytes, i, '`');
                    if run >= 3 {
                        if in_code_fence {
                            if fence_char == '`' && fence_len == run {
                                in_code_fence = false;
                            }
                        } else {
                            in_code_fence = true;
                            fence_char = '`';
                            fence_len = run;
                        }
                        i += run;
                    } else if !in_code_fence {
                        in_inline_code = !in_inline_code;
                        i += run;
                    } else {
                        i += 1;
                    }
                }
                '~' => {
                    let run = count_run(bytes, i, '~');
                    if run >= 3 {
                        if in_code_fence {
                            if fence_char == '~' && fence_len == run {
                                in_code_fence = false;
                            }
                        } else {
                            in_code_fence = true;
                            fence_char = '~';
                            fence_len = run;
                        }
                        i += run;
                    } else {
                        i += 1;
                    }
                }
                '*' => {
                    if in_code_fence || in_inline_code {
                        i += 1;
                        continue;
                    }
                    let run = count_run(bytes, i, '*');
                    if run >= 2 {
                        in_bold = !in_bold;
                        i += run;
                    } else {
                        in_italic = !in_italic;
                        i += 1;
                    }
                }
                '_' => {
                    if in_code_fence || in_inline_code {
                        i += 1;
                        continue;
                    }
                    let run = count_run(bytes, i, '_');
                    if run >= 2 {
                        in_bold = !in_bold;
                        i += run;
                    } else {
                        in_italic = !in_italic;
                        i += 1;
                    }
                }
                '[' => {
                    if !in_code_fence && !in_inline_code {
                        in_link = true;
                    }
                    i += 1;
                }
                ')' => {
                    if in_link && !in_code_fence && !in_inline_code {
                        in_link = false;
                    }
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }

            if !in_code_fence && !in_inline_code && !in_bold && !in_italic && !in_link {
                last_clean = i;
            }
        }

        last_clean
    }
}

fn count_run(bytes: &[u8], start: usize, c: char) -> usize {
    let mut n = 0;
    let mut i = start;
    while i < bytes.len() && bytes[i] as char == c {
        n += 1;
        i += 1;
    }
    n
}

#[cfg(test)]
#[path = "markdown_buffer_test.rs"]
mod markdown_buffer_test;
