//! `suture init`: a directory a coding agent can start in (appendix AO.2.5).
//! It writes what is missing and never overwrites a file.

use std::path::Path;

/// What a coding agent reads first, in any tool that reads AGENTS.md.
pub const AGENTS: &str = "# Working on this Suture module

This directory is a Suture module: a backend written as rules, permissions, needs and effects, in controlled English.
You have probably not seen Suture before. Do not guess its syntax from other languages; look it up.

1. Read `suture help` once: the core, about 2k tokens, with every form, recipes and limits.
   Before you edit this module, run `suture help --for .` for the details of the forms it uses,
   and `suture help ⟨topic⟩` for any other. Check `limits` before you decide something cannot be written.
2. Write the `.st` files and a `.scenario` file per topic, in this directory.
3. Run `suture check .` after every edit. It compiles, runs every scenario, and shows only what failed.
   Exit 0 means everything compiles and passes. Fix the first error first; each error names its topic.
4. Cover every requirement with a scenario, and every limit or permission with a refused one
   (`is refused by rule ⟨name⟩`, `is refused at the gate`).
5. When it passes, run `suture build .` and give the person its read-back: every rule in plain English.
   That read-back, and the scenarios, are what they review, so keep rule names meaningful.
";

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
    ];
    if !has_module {
        files.push((format!("{name}.st"), module));
        files.push((format!("{name}.scenario"), SCENARIO.into()));
    }
    let mut out = String::new();
    for (file, text) in files {
        let path = dir.join(&file);
        if path.symlink_metadata().is_ok() {
            out += &format!("  kept    {file}\n");
            continue;
        }
        // `create_new`, so a file that appeared meanwhile is never overwritten.
        let file_ = std::fs::OpenOptions::new().write(true).create_new(true).open(&path);
        std::io::Write::write_all(&mut file_.map_err(|e| format!("{}: {e}", path.display()))?, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
        out += &format!("  wrote   {file}\n");
    }
    out += &format!("next: `suture check {}`\n", dir.display());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_says_what_agents_md_says() {
        let skill = include_str!("../../docs/agents/skill/suture/SKILL.md");
        assert!(skill.starts_with("---\nname: suture\n"));
        assert!(skill.ends_with(AGENTS), "docs/agents/skill/suture/SKILL.md must end with init.rs's AGENTS text");
    }

    #[test]
    fn init_writes_what_is_missing_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("suture-init-{}", std::process::id())).join("rooms");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "mine").unwrap();
        let out = init(&dir).unwrap();
        assert!(out.contains("kept    AGENTS.md") && out.contains("wrote   rooms.st") , "{out}");
        assert_eq!(std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(), "mine");
        let found = crate::check::check(&dir);
        assert!(found.passed(), "{}", found.text());
        // A module already there gets no starter.
        std::fs::remove_file(dir.join("rooms.scenario")).unwrap();
        assert!(!init(&dir).unwrap().contains("rooms.scenario"));
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }
}
