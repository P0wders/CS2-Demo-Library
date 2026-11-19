# CS2 Demo Library

Use `cs2_demo` to read Counter-Strike 2 demo files from Rust. Choose a starting point based on what your application needs to keep.

| I’m building… | Start with | Work performed |
|---|---|---|
| A match list | [Getting started](Getting-started.md) | Read metadata, then release the file mapping. |
| A scoreboard or round breakdown | [Match statistics](Match-statistics.md) | Parse once and query the resulting match. |
| A kill feed or event counter | [Game events](Game-events.md) | Stream recorded events without decoding entity updates. |
| A 2D radar viewer | [Replay viewer](Replay-viewer.md) | Sample entity positions and retain frames at your chosen interval. |

The library reads local files. A website can use it in a Rust backend and send only the requested results to the browser.
