---
name: suture
description: Use when writing, changing or debugging a Suture module (.st files) or its scenarios (.scenario files), or when asked to build a backend in Suture.
---

# Working on this Suture module

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
