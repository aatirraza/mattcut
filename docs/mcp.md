# Connect Claude to MattCut (MCP)

`mattcut-mcp` is an MCP server (stdio). With the MattCut app open, Claude edits **your open
project live**: you watch the timeline change, and Claude can take screenshots of the player.
Without the app it works on a project file headlessly.

## Claude Code

```sh
claude mcp add mattcut -- /Users/mac/Desktop/mattpremier-pro/mattcraft/target/release/mattcut-mcp
```

On Windows: `claude mcp add mattcut -- C:\MattCut\mattcut-mcp.exe`.

## Claude Desktop

Settings ▸ Developer ▸ Edit Config, add:

```json
{
  "mcpServers": {
    "mattcut": {
      "command": "/Users/mac/Desktop/mattpremier-pro/mattcraft/target/release/mattcut-mcp"
    }
  }
}
```

Restart Claude Desktop.

## How it connects

The app listens on `127.0.0.1` only (port 9877 or the next free one) and writes the port and a
random token to `control.json` in MattCut's data folder (owner-only):

* macOS: `~/Library/Application Support/MattCut/control.json`
* Windows: `%APPDATA%\MattCut\control.json`

`mattcut-mcp` reads it, so there is nothing to configure. The app refuses MCP edits while it is
locked (off the office network), like any other edit.

Headless mode: `mattcut-mcp --headless --project /path/to/project.fcproj` (save with the
`file.save` command).

## Tools

| Tool | Use |
|---|---|
| `command_list` | Every editing command (700+): id, parameters, shortcut |
| `command_run` / `command_batch` | Run commands, e.g. `timeline.razor`, `timeline.place`, `effects.apply`, `clip.speedDuration`, `graphics.newText`, `captions.add`, `file.exportMedia` |
| `doc_inspect`, `project_inspect`, `sequence_inspect` | Read the project, tracks and clips |
| `media_import` | Import files by path |
| `render_frame` / `render_preview` | See a frame as an image |
| `ui_screenshot` | Screenshot of the MattCut window (or `panel: "Program"` for the player) |
| `ui_inspect` | App state: project, playhead, selection, office status |
| `ui_control` | `ui.playback` (`{"action":"play"|"pause"|"toggle"}`) |

Example prompts:

* "Import ~/Movies/shoot/*.mp4, put them on the main track in name order, trim each to 3
  seconds, add cross dissolves, and export a 1080×1920 MP4 to ~/Desktop."
* "Add a big white outlined title 'SUMMER SALE' for the first 2 seconds."
* "Make the second clip 2× speed and lower the music on A2 to -12 dB."
