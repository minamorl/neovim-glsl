# TRACEABILITY — pin id と成果物の対応

`pins/domains/neovim-glsl.spec@0.5` を `pins/house_style.pin@1.8` まで flatten した pin と、この repository のどこがそれを満たすかの対応表。1 行で辿れることが目的 (`impl.traceable`)。

v0.5 の人間ゲート回答 `relax` で退役した pin は、行を消さず `RETIRED@0.5` として残す。設立時に何を約束していたかと、いつ誰が緩めたかの両方が 1 行で辿れる必要があるため。

内訳: domain pin 7 (うち 2 は RETIRED@0.5) + v0.5 で追加 2、property 2 (両方 RETIRED@0.5) + v0.5 で追加 1、example 4 (うち 1 は RETIRED@0.5) + v0.5 で追加 3、house_style から継承した pin 28。

表の cell は機械処理しやすいよう ASCII のみ。補足は表の下の注記に置く。

## domain pin (7)

| pin id                                 | statement                                               | honored at                                                                        |
| -------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------- |
| neovim_glsl.editor_basis               | RETIRED@0.5 (was: require editor.basis = neovim)        | founding/record.json superseded_at_v0_5.retired[0]; spec v0.5 free editor_basis   |
| neovim_glsl.no_editor_substitution     | RETIRED@0.5 (was: forbid editor.substitution = allowed) | founding/record.json superseded_at_v0_5.retired[1]                                |
| neovim_glsl.neovim_asset_not_discarded | forbid neovim_glsl.neovim_assets.disposition = discarded| evaluation/ が Neovim を embed して動いていること自体; spec v0.5                  |
| neovim_glsl.neovim_retention_mode      | require neovim.retention_mode = editing_experience_preservation | evaluation/candidate-embed-opengl (nvim の keymap/編集意味論をそのまま通す) |
| neovim_glsl.emacs_alternative_rejected | forbid neovim_glsl.editor.basis = emacs_family          | founding/record.json frozen_pins[2]; README.md (frozen)                           |
| neovim_glsl.project_established        | require neovim_glsl.project.establishment = required    | this repository as a whole; founding/record.json frozen_pins[3] and establishment |
| neovim_glsl.project_subject            | require neovim_glsl.project.subject = neovim_to_glsl    | founding/record.json frozen_pins[4] and establishment.subject; README.md (title)  |
| neovim_glsl.establishment_order        | require neovim_glsl.project.establishment.order = first | founding/record.json frozen_pins[5] and establishment.order; README.md (frozen)   |
| neovim_glsl.target_shading_language    | require neovim_glsl.target.shading_language = glsl      | founding/record.json frozen_pins[6]; README.md (frozen)                           |

## property と example (2 + 4)

| id                                  | kind     | statement                                                          | honored at                                                    |
| ----------------------------------- | -------- | ------------------------------------------------------------------ | ------------------------------------------------------------- |
| neovim_glsl.editor_basis_witness    | property | RETIRED@0.5 (was: forall stage . editor_basis(stage) == neovim)    | founding/record.json superseded_at_v0_5.retired[2]            |
| neovim_glsl.no_substitution_witness | property | RETIRED@0.5 (was: forall candidate . editor_substitution == rejected) | founding/record.json superseded_at_v0_5.retired[3]         |
| neovim_glsl.retention_witness       | property | forall stage . neovim_assets_disposition(stage) != discarded       | evaluation/ が nvim --embed を使い続けていること               |
| neovim_glsl.editor_retained         | example  | RETIRED@0.5 (was: editor_basis_choice => neovim)                   | founding/record.json superseded_at_v0_5.retired[4]            |
| neovim_glsl.basis_free              | example  | non_neovim_host_proposal => not_rejected_by_basis_pin              | spec v0.5 (この repository はまだ別 host を建てていない)       |
| neovim_glsl.asset_kept              | example  | proposal_discarding_all_neovim_assets => rejected                  | spec v0.5                                                      |
| neovim_glsl.emacs_still_rejected    | example  | emacs_family_editor_substitution => rejected                       | spec v0.5; emacs_alternative_rejected は退役していない          |
| neovim_glsl.emacs_alternative       | example  | emacs_family_editor_substitution => rejected                       | founding/record.json witnesses[1]; README.md                  |
| neovim_glsl.founding                | example  | this_instruction_deliverable => established_neovim_to_glsl_project | founding/record.json witnesses[2]; this repository; README.md |
| neovim_glsl.shading_language        | example  | project_target_shading_language => glsl                            | founding/record.json witnesses[3]; README.md                  |

## house_style@1.8 から継承した pin (28)

| pin id                       | statement                                                            | status                             | honored at                                                          |
| ---------------------------- | -------------------------------------------------------------------- | ---------------------------------- | ------------------------------------------------------------------- |
| error.envelope               | require error.envelope = { code, message, details, trace_id }        | binding_no_subject_yet             | founding/record.json                                                |
| error.http_shape             | require error.body.field in [code, message, details]                 | binding_no_subject_yet             | founding/record.json                                                |
| error.no_raw_stack           | forbid error.expose = raw_stacktrace                                 | witnessed_by_absence               | founding/record.json                                                |
| log.format                   | require log.format = json                                            | binding_no_subject_yet             | founding/record.json                                                |
| log.required_fields          | require log.fields in [ts, level, trace_id, msg]                     | binding_no_subject_yet             | founding/record.json                                                |
| log.no_secrets               | forbid log.contains = secret                                         | witnessed_by_absence               | founding/record.json                                                |
| secret.source                | require secret.source = env_or_vault                                 | binding_no_subject_yet             | .gitignore; founding/record.json                                    |
| secret.no_commit             | forbid secret.location = repo                                        | witnessed_by_absence               | .gitignore; founding/record.json                                    |
| idempotency.write            | require write.idempotency = required                                 | binding_no_subject_yet             | founding/record.json                                                |
| retry.max                    | range retry.max = 0..5                                               | binding_no_subject_yet             | founding/record.json                                                |
| retry.backoff                | require retry.backoff = exponential_jitter                           | binding_no_subject_yet             | founding/record.json                                                |
| time.tz                      | require time.storage = utc                                           | witnessed_here                     | founding/record.json recorded_at and time_storage                   |
| id.scheme                    | require id.scheme in [uuidv7, ulid]                                  | witnessed_here                     | founding/record.json record_id and record_id_scheme                 |
| spec.autosave_gate           | require spec.session_end.gate = spec_autosave                        | binding_declared_upstream_enforced | founding/record.json                                                |
| spec.require_update_flag     | require spec.require_update.trigger = env_SPEC_REQUIRE_UPDATE        | binding_declared_upstream_enforced | founding/record.json                                                |
| spec.hook_no_push            | forbid spec.hook.git_push = enabled                                  | witnessed_by_absence               | no hook defined; founding commit not pushed; README.md (no push)    |
| spec.boot.scope              | require spec.boot.scope = every_ai_chat_session                      | binding_declared_upstream_enforced | founding/record.json; README.md                                     |
| spec.boot.agents             | require spec.boot.agent in [codex, claude_code, yui]                 | binding_declared_upstream_enforced | founding/record.json                                                |
| spec.boot.user_reminder      | forbid spec.boot.user_reminder = required                            | binding_declared_upstream_enforced | founding/record.json; README.md (dev entry)                         |
| spec.boot.cwd                | require spec.boot.cwd = independent                                  | binding_declared_upstream_enforced | founding/record.json; README.md (dev entry)                         |
| spec.boot.development_entry  | require spec.development.first_action = spec_system_supervisor_route | witnessed_here                     | README.md (dev entry); founding/record.json                         |
| spec.boot.missing_route      | require spec.development.missing_or_red_spec = blocked               | binding_declared_upstream_enforced | README.md (dev entry); founding/record.json                         |
| spec.boot.generator_context  | require spec.generator.context = fresh_spec_only                     | binding_declared_upstream_enforced | README.md (dev entry); founding/record.json                         |
| spec.boot.supervisor_context | require spec.supervisor.yui_context = required_before_judgement      | binding_declared_upstream_enforced | README.md (dev entry); founding/record.json                         |
| spec.boot.prompt_source      | require spec.boot.prompt.source = house_style_boot_contract          | witnessed_here                     | README.md (dev entry, reference only no copy); founding/record.json |
| spec.boot.no_fallback        | forbid spec.development.code_first_fallback = allowed                | witnessed_by_absence               | no artifact produced outside the spec route; founding/record.json   |
| ci.status_green              | require ci.status = green                                            | binding_no_subject_yet             | founding/record.json; README.md (not frozen)                        |
| ci.format_prettier           | require ci.format.gate = prettier                                    | binding_no_subject_yet             | founding/record.json; README.md (not frozen)                        |

### status の意味

- `witnessed_here` — この record または repository が今そのまま満たしている。
- `witnessed_by_absence` — forbid 型。禁止された事象がこの repository に存在しない。
- `binding_declared_upstream_enforced` — spec-system の session と台帳を縛る pin。この repository は拘束として宣言し、矛盾するものを一切定義しない。
- `binding_no_subject_yet` — require 型。値を担う component が設立段階に存在しない。

### binding_no_subject_yet に code が無い理由

`error.*`、`log.format`、`log.required_fields`、`idempotency.write`、`retry.*`、`secret.source`、`ci.*` は継承した拘束として有効だが、設立段階にはそれを担う component が無い。ここで error envelope や logger や retry 層を書けば、

- spec が凍結していない振る舞いを足すことになり `impl.no_overreach` に反し、
- host 実装言語・runtime・build system・CI provider・package manager を選ぶことになり `free neovim_glsl.host_implementation_language` と `free neovim_glsl.project_form` を暗黙 pin 化する (`impl.free_not_locked` に反する)。

したがって「拘束として記録し、担い手を作らない」が唯一 pin を 1 つも壊さない扱いである。component を作る時点でこれらを満たす。

## 未決 (実装しない) と free (固定しない)

| group         | count | treatment                 | recorded at                                                                        |
| ------------- | ----- | ------------------------- | ---------------------------------------------------------------------------------- |
| quarantine    | 5     | not implemented; verbatim | UNDECIDED.md section quarantine; founding/record.json not_decided.quarantine       |
| open_question | 9     | not implemented; verbatim | UNDECIDED.md section open_question; founding/record.json not_decided.open_question |
| free          | 11    | not locked; verbatim      | DESIGN-SPACE.md; founding/record.json not_locked.free                              |

未決を実装しないこと自体が `impl.defer_open` の遵守であり、free を固定しないこと自体が `impl.free_not_locked` の遵守である。この repository に architecture・GLSL 化範囲・性能基準・shader・build 設定が無いのは欠落ではない。

`evaluation/` 配下の性能測定はこの行を変えない。あそこにあるのは観測であって基準ではなく、
`quarantine neovim_glsl.performance_criteria` と `free neovim_glsl.performance.numeric_targets`
は依然として空である。閾値は実行時に渡されたときだけ report に入り、既定では
`unset_awaiting_human_gate` と明記されて出る。

## 正本

`spec` が真実。この表と spec が食い違ったら spec が勝つ。表と repository を直し、pin を緩めない (`impl.spec_is_truth`)。
