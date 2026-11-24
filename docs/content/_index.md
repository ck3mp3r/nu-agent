+++
title = "nu-agent documentation"
description = "Documentation for the nu-agent Nushell plugin."
sort_by = "weight"
template = "index.html"

[extra]
lead = "nu-agent is a Nushell plugin that runs an LLM agent from a pipeline. It reads files, runs tools, edits code — and you watch every step in a replayable session."
url = "/docs/getting-started/quick-start/"
url_button = "Run your first agent"
repo_url = "https://github.com/ck3mp3r/nu-agent"
repo_license = "MIT"
repo_version = "GitHub v0.1.0"

[[extra.list]]
title = "Runs from the shell you already use"
content = "The prompt is a pipeline: feed it a string or a record, stream the answer, keep Nushell types end to end."

[[extra.list]]
title = "Built-in tools, not a chat window"
content = "read, edit, grep, glob, http, nu, and four tree-sitter queries in the binary. MCP servers bolt on everything else."

[[extra.list]]
title = "Sessions with receipts"
content = "Turns persist to SQLite, transcripts replay with their tool previews, and a running-out context is compacted without losing the thread."

[[extra.list]]
title = "One config file"
content = "Model, price per million tokens, permission per tool, and user hooks in Nushell — all in ~/.config/nu-agent/config.toml."
+++
