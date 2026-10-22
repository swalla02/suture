//! `suture init`: a directory a coding agent can start in (appendix AO.2.5).
//! It writes what is missing and never overwrites a file.

use std::path::Path;

/// What a coding agent reads first, in any tool that reads AGENTS.md.
pub const AGENTS: &str = "# Working on this Suture module

This directory is a Suture module: a backend written as rules, permissions, needs and effects, in controlled English.
You have probably not seen Suture before. Do not guess its syntax from other languages; look it up.

1. `suture help` lists the cards; read `suture help start` and `suture help recipes` first, and `suture help limits`
   before you decide something cannot be written. `suture help all` prints every card (about 450 lines).
2. Write the `.st` files and a `.scenario` file per topic, in this directory.
3. Run `suture check .` after every edit. It compiles, runs every scenario, and shows only what failed.
   Exit 0 means everything compiles and passes. Fix the first error first; each error names the form it expected.
4. Cover every requirement with a scenario, and every limit or permission with a refused one
   (`is refused by rule ⟨name⟩`, `is refused at the gate`).
5. When it passes, run `suture build .` and give the person its read-back: every rule in plain English.
   That read-back, and the scenarios, are what they review, so keep rule names meaningful.

With MCP, the same commands are the tools `suture_help`, `suture_check`, `suture_build` and `suture_explain` (`suture mcp`).
";

/// The MCP server, for Claude Code and other clients that read `.mcp.json`.
const MCP: &str = "{\n  \"mcpServers\": {\n    \"suture\": { \"command\": \"suture\", \"args\": [\"mcp\"] }\n  }\n}\n";

const MODULE: &str = "role staff

kind Room    { name: Text, seats: Count }
kind Booking { room: Room, people: Count, cancelled: Bool = no }

rule fits: every Booking's people is at most its room's seats

when a staff cancels a Booking: the Booking is cancelled

admit add a new Room by staff
admit book a new Booking by staff
admit cancel Booking by staff
";

const SCENARIO: &str = "scenario \"a booking that does not fit is refused\":
  given  a Room R with name \"Oak\" and seats 4
  when   staff Sam books a new Booking B1 with room R and people 3
  and    staff Sam books a new Booking B2 with room R and people 6
         is refused by rule fits
  expect B1's people is 3
";

/// The module's name: the directory's, if it is a lower-case word.
fn module_name(dir: &Path) -> String {
    let name = std::fs::canonicalize(dir).ok().and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_lowercase())).unwrap_or_default();
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase()) { name } else { "app".to_string() }
}

/// Writes the files that are missing, and says which.
pub fn init(dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let has_module = std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten().any(|e| e.path().extension().is_some_and(|x| x == "st"));
    let name = module_name(dir);
    let module = format!("module {name} version 1, currency EUR\n\n{MODULE}");
    let mut files: Vec<(String, String)> = vec![
        ("AGENTS.md".into(), AGENTS.into()),
        ("CLAUDE.md".into(), "@AGENTS.md\n".into()),
        (".mcp.json".into(), MCP.into()),
    ];
    if !has_module {
        files.push((format!("{name}.st"), module));
        files.push((format!("{name}.scenario"), SCENARIO.into()));
    }
    let mut out = String::new();
    for (file, text) in files {
        let path = dir.join(&file);
        if path.exists() {
            out += &format!("  kept    {file}\n");
            continue;
        }
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        out += &format!("  wrote   {file}\n");
    }
    out += &format!("next: `suture check {}`\n", dir.display());
    Ok(out)
}
