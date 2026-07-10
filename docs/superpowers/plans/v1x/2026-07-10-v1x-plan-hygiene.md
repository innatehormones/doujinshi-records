# V1.x Sub-Plan 7 — V1 Plan Hygiene

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Implements umbrella candidate **#7**.

**Goal:** Re-evaluate each of the 107 outstanding `- [ ]` items in `docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md` and tick the ones that the codebase already satisfies. Leave the rest as `- [ ]` with an inline note pointing at the relevant V1.x sub-plan that closes them.

**Architecture:** A small PowerShell script automates the bulk edit; the engineer reviews the diff before committing.

**Tech Stack:** `Select-String` + a manual pass.

---

## Task 1: Catalogue the open checkboxes

- [ ] **Step 1: Get a numbered list**

Run: `Select-String -Path docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md -Pattern '^- [ ]' | ForEach-Object { "{0}: {1}" -f $_.LineNumber, ($_.Line.Substring(0, [Math]::Min(120, $_.Line.Length))) } > /tmp/open.txt`
Expected: `/tmp/open.txt` has 107 numbered entries.

- [ ] **Step 2: Walk the plan with `git log`**

For each Task N, confirm whether the step's content has been delivered in a commit. Use:
`git log --oneline -- <files-touched-by-step>`
- If a matching commit exists in master → mark `- [x]`.
- If no matching commit and no V1.x plan covers it → leave `- [ ]`.
- If a V1.x plan covers it → leave `- [ ]` with a `<!-- see v1x/2026-07-10-v1x-X.md -->` annotation.

---

## Task 2: Apply the changes

**Files:**
- Modify: `docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md`

- [ ] **Step 1: Bulk tick**

For each "implemented" item, change the leading `- [ ]` to `- [x]` using `replace_all` is **not** safe (would tick everything). Instead, use the PowerShell snippet:

```powershell
$lines = Get-Content docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md
$targets = @(45, 47, 123, ...)  # line numbers from Task 1 step 1
foreach ($i in $targets) { $lines[$i - 1] = $lines[$i - 1] -replace '^- [ ]', '- [x]' }
Set-Content -LiteralPath docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md -Value $lines -Encoding UTF8
```

(Fill in `$targets` with the actual line numbers after the catalogue pass.)

- [ ] **Step 2: Annotate V1.x-handoff items**

For each line that should be deferred, change to:
```markdown
- [ ] **Step K: ...** <!-- see v1x/2026-07-10-v1x-XXX.md -->
```

- [ ] **Step 3: Verify the diff**

Run: `git diff --stat docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md`
Expected: < 200 line changes; the file is still readable.

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/plans/2026-07-09-doujinshi-records-v1.md
git commit -m "docs(plan): tick implemented steps; annotate V1.x handoffs"
```

---

## Self-review

- [ ] Diff is line-numbered (use `git diff -U0` to confirm) so the reviewer can audit each tick.
- [ ] No step was ticked that the code does not actually do (spot-check 3-5 random ticks against `git log`).
- [ ] All eight umbrella candidates are cross-referenced via the `v1x/` annotations.
