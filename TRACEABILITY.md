# TRACEABILITY — pin id と成果物の対応

`pins/domains/neovim-glsl.spec@0.9` を `pins/house_style.pin@1.8` まで flatten した pin と、この repository のどこがそれを満たすかの対応表。1 行で辿れることが目的 (`impl.traceable`)。

この表は生成物ではない。3 列目「honored at」は *この repository* についての主張であり、spec のどの行にも書かれていないため。機械で確かめられるのは「spec の pin id が漏れなく表に現れているか」だけで、それは `python3 tools/check_traceability.py <spec-or-mirror>` が見る。v0.8 を丸ごと取り落とした（`@0.7` のまま、domain pin 14 のまま、locus pin 5 行が不在）ことに誰も気づかなかったので、次は黙って腐らず落ちる。

v0.5 の人間ゲート回答 `relax`、v0.6 の `own_host_protocol`、v0.7 の「telescope である必然性は特にない」、v0.8 の `glsl_overlay` で退役・解決した行は、行を消さず `RETIRED@X.Y` / `RESOLVED@X.Y` として残す。設立時に何を約束していたかと、いつ誰が緩めたかの両方が 1 行で辿れる必要があるため。

v0.9 は pin を一つも増やしていない。人間ゲートが `navigation_state_owner` に「わからない」を返したためで、不明は選択ではないので要求にならない。この版で表に増える行は無く、増えたのは「観測待ち」という問いの状態だけである。

spec 側の pin / property / example は 93 件。内訳を手で数えるのはやめた: 数えた瞬間に spec との二重管理になり、実際 v0.2・v0.3・v0.4 の 47 件がこの表から丸ごと落ちていたのに、内訳の数字は v0.7 まで整合しているように見えていた。網羅は `python3 tools/check_traceability.py` が数える。house_style@1.8 から継承した pin 28 は別表。

「witness pending」と書いてある行は、この repository がまだその pin を満たしていないという意味である。嘘の trace を書けば網羅チェックは通ってしまうので、無い witness は無いと書く。

表の cell は機械処理しやすいよう ASCII のみ。補足は表の下の注記に置く。

## domain pin (19)

| pin id                                 | statement                                               | honored at                                                                        |
| -------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------- |
| neovim_glsl.editor_basis               | RETIRED@0.5 (was: require editor.basis = neovim)        | founding/record.json superseded_at_v0_5.retired[0]; spec v0.5 free editor_basis   |
| neovim_glsl.no_editor_substitution     | RETIRED@0.5 (was: forbid editor.substitution = allowed) | founding/record.json superseded_at_v0_5.retired[1]                                |
| neovim_glsl.neovim_asset_not_discarded | forbid neovim_glsl.neovim_assets.disposition = discarded| spec v0.6; protocol reuse is pinned; all-asset discard remains forbidden           |
| neovim_glsl.neovim_retention_mode      | require neovim.retention_mode = editing_experience_preservation | evaluation/candidate-embed-opengl records the measured Neovim editing path |
| neovim_glsl.editor_basis_own_host      | require neovim_glsl.editor.basis = own_host             | witness pending - no own-host artefact yet; README.md records selected architecture |
| neovim_glsl.architecture_choice        | require neovim_glsl.architecture = own_host_speaking_neovim_protocol | witness pending - no own-host artefact yet; README.md and founding/record.json superseded_at_v0_6 |
| neovim_glsl.host_protocol_dialect      | require neovim_glsl.host.protocol.dialect = neovim      | witness pending - no protocol-speaking own host session yet                         |
| neovim_glsl.asset_reuse_includes_protocol | require neovim_glsl.neovim_assets.reuse_scope includes protocol | README.md; UNDECIDED.md narrowed neovim_asset_reuse_scope                         |
| neovim_glsl.file_navigation_mechanism  | RETIRED@0.7 (was: require file_navigation.mechanism = telescope) | founding/record.json superseded_at_v0_7.retired[0]                       |
| neovim_glsl.file_navigation_required   | require neovim_glsl.file_navigation = required          | evaluation/free-surface measures a surface for it; no mechanism chosen yet        |
| neovim_glsl.navigation_locus_choice    | require neovim_glsl.navigation.ui_locus = glsl_surface_over_grid | evaluation/free-surface/out/locus.json; src/surface_locus.rs; verify.py checks it against the pixels |
| neovim_glsl.navigation_not_in_grid     | forbid neovim_glsl.navigation.ui_locus = terminal_grid  | src/surface_locus.rs keeps the forbidden loci nameable so a violation can be recorded |
| neovim_glsl.navigation_not_separate_window | forbid neovim_glsl.navigation.ui_locus = separate_os_window | same vertex buffer as the grid; a separate window could not append to it        |
| neovim_glsl.navigation_not_separate_process | forbid neovim_glsl.navigation.ui_locus = separate_process | same vertex buffer as the grid; measured 36366 -> 39090 vertices               |
| neovim_glsl.navigation_surface_renderer | require neovim_glsl.navigation.surface.renderer = host  | evaluation/free-surface/out/locus.json emitter is this process                     |
| neovim_glsl.emacs_alternative_rejected | forbid neovim_glsl.editor.basis = emacs_family          | founding/record.json frozen_pins[2]; README.md (frozen)                           |
| neovim_glsl.project_established        | require neovim_glsl.project.establishment = required    | this repository as a whole; founding/record.json frozen_pins[3] and establishment |
| neovim_glsl.project_subject            | require neovim_glsl.project.subject = neovim_to_glsl    | founding/record.json frozen_pins[4] and establishment.subject; README.md (title)  |
| neovim_glsl.establishment_order        | require neovim_glsl.project.establishment.order = first | founding/record.json frozen_pins[5] and establishment.order; README.md (frozen)   |
| neovim_glsl.target_shading_language    | require neovim_glsl.target.shading_language = glsl      | founding/record.json frozen_pins[6]; README.md (frozen)                           |

## property と example (8 + 24)

| id                                  | kind     | statement                                                          | honored at                                                    |
| ----------------------------------- | -------- | ------------------------------------------------------------------ | ------------------------------------------------------------- |
| neovim_glsl.editor_basis_witness    | property | RETIRED@0.5 (was: forall stage . editor_basis(stage) == neovim)    | founding/record.json superseded_at_v0_5.retired[2]            |
| neovim_glsl.no_substitution_witness | property | RETIRED@0.5 (was: forall candidate . editor_substitution == rejected) | founding/record.json superseded_at_v0_5.retired[3]         |
| neovim_glsl.retention_witness       | property | forall stage . neovim_assets_disposition(stage) != discarded       | protocol reuse pin; evaluation/ remains measured Neovim evidence |
| neovim_glsl.host_protocol_witness   | property | forall session . host_protocol_dialect(session) == neovim          | witness pending - no protocol-speaking own host session yet    |
| neovim_glsl.basis_witness           | property | forall stage . editor_basis(stage) == own_host                     | witness pending - no own-host stage artefact yet               |
| neovim_glsl.editor_retained         | example  | RETIRED@0.5 (was: editor_basis_choice => neovim)                   | founding/record.json superseded_at_v0_5.retired[4]            |
| neovim_glsl.basis_free              | example  | RETIRED@0.6 (was: non_neovim_host_proposal => not_rejected_by_basis_pin) | founding/record.json superseded_at_v0_6.retired[0]       |
| neovim_glsl.asset_kept              | example  | proposal_discarding_all_neovim_assets => rejected                  | spec v0.6; asset_reuse_includes_protocol                       |
| neovim_glsl.emacs_still_rejected    | example  | emacs_family_editor_substitution => rejected                       | spec v0.6; emacs_alternative_rejected は退役していない          |
| neovim_glsl.host_choice             | example  | editor_host_selection => own_host_speaking_neovim_protocol         | founding/record.json superseded_at_v0_6.added; README.md      |
| neovim_glsl.embed_not_selected      | example  | neovim_core_process_as_editor_basis => not_selected                | founding/record.json superseded_at_v0_6.added; README.md      |
| neovim_glsl.full_scratch_not_selected | example | host_discarding_neovim_protocol => rejected                        | founding/record.json superseded_at_v0_6.added; README.md      |
| neovim_glsl.protocol_asset          | example  | neovim_protocol_as_reused_asset => accepted                        | founding/record.json superseded_at_v0_6.added; README.md      |
| neovim_glsl.navigation_presence_witness | property | forall surface . navigation_mechanism_present(surface) == true | witness pending - no navigation mechanism built yet        |
| neovim_glsl.navigation              | example  | RETIRED@0.7 (was: file_move_operation => telescope)                | founding/record.json superseded_at_v0_7.retired[1]            |
| neovim_glsl.non_telescope_navigation | example | non_telescope_navigation_proposal => not_rejected_by_navigation_pin | evaluation/free-surface is such a proposal and is not rejected |
| neovim_glsl.telescope_still_allowed | example  | telescope_as_navigation_mechanism => not_rejected                  | evaluation/protocol-surface keeps the telescope measurement live |
| neovim_glsl.external_glsl_picker    | example  | picker_rendered_outside_grid_via_glsl => not_rejected              | evaluation/free-surface/out/free-surface-over-grid.png        |
| neovim_glsl.navigation_removal      | example  | proposal_with_no_navigation_mechanism => rejected                  | witnessed by absence - nothing here removes navigation        |
| neovim_glsl.emacs_alternative       | example  | emacs_family_editor_substitution => rejected                       | founding/record.json witnesses[1]; README.md                  |
| neovim_glsl.founding                | example  | this_instruction_deliverable => established_neovim_to_glsl_project | founding/record.json witnesses[2]; this repository; README.md |
| neovim_glsl.shading_language        | example  | project_target_shading_language => glsl                            | founding/record.json witnesses[3]; README.md                  |
| neovim_glsl.navigation_locus_witness | property | forall picker_frame . navigation_surface_locus(picker_frame) == glsl_surface_over_grid | src/surface_locus.rs reports it from the built frame        |
| neovim_glsl.navigation_renderer_witness | property | forall picker_frame . navigation_surface_renderer(picker_frame) == host | src/surface_locus.rs rejects a locus claimed by a surface that emitted nothing |
| neovim_glsl.navigation_overlay      | example  | picker_drawn_as_glsl_surface_over_grid => accepted                 | evaluation/free-surface/out/free-surface-over-grid.png         |
| neovim_glsl.navigation_in_grid_rejected | example | picker_drawn_into_terminal_grid => rejected                     | witnessed by absence - nothing here draws a picker into cells  |
| neovim_glsl.navigation_separate_window_rejected | example | picker_in_separate_os_window => rejected                | witnessed by absence - one window only                         |
| neovim_glsl.navigation_plugin_window_rejected | example | plugin_drawing_its_own_floating_grid_as_the_picker_surface => rejected | evaluation/state-ownership draws nothing plugin-side; it only reads state |
| neovim_glsl.navigation_source_still_free | example | plugin_supplying_rows_to_a_host_drawn_surface => not_rejected  | evaluation/state-ownership measures exactly that arrangement    |
| neovim_glsl.unknown_gate_adds_no_requirement | example | gate_answer_unknown_recorded_without_new_pin => accepted   | spec v0.9 holds at 80 pins; UNDECIDED.md keeps the question open |
| neovim_glsl.unknown_gate_not_read_as_free | example | gate_answer_unknown_relabelled_as_free_axis => rejected       | navigation_state_owner is still open_question, not free         |
| neovim_glsl.unknown_gate_not_defaulted | example | picker_state_owner_pinned_without_a_human_choice => rejected      | no pin names a state owner; both halves are evidence only      |
| neovim_glsl.measurement_may_inform_without_deciding | example | state_ownership_measured_while_question_stays_open => accepted | evaluation/state-ownership/compare.py fails if either half claims a decision |

## domain pin — v0.2 aish 統合 (6)

| pin id                                 | statement                                               | honored at                                                                        |
| -------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------- |
| neovim_glsl.aish_integration           | require neovim_glsl.integration.aish = required         | evaluation/candidate-embed-opengl/src/aish.rs (read-only structured surface)       |
| neovim_glsl.aish_integration_counterpart | require neovim_glsl.integration.counterpart = ai_native_shell | src/aish.rs speaks to aish and to nothing else                             |
| neovim_glsl.aish_integration_commencement | require neovim_glsl.integration.aish.commencement = required | src/aish.rs exists; commencement is what is pinned, not completion        |
| neovim_glsl.aish_repository_private    | require neovim_glsl.integration.counterpart_repository.visibility = private | outside this repository; aish is a separate private repository  |
| neovim_glsl.aish_repository_not_public | forbid neovim_glsl.integration.counterpart_repository.visibility = public | same; nothing here publishes aish                                |
| neovim_glsl.aish_repository_private_first | require neovim_glsl.integration.counterpart_repository.private_setup.order = before_integration | aish was private before src/aish.rs was written |

## domain pin — v0.3 platform と portability (6)

| pin id                                 | statement                                               | honored at                                                                        |
| -------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------- |
| neovim_glsl.first_stage_platform       | require neovim_glsl.stage.first.platform = mac           | every measurement in evaluation/ was taken on Apple M4 Max / macOS 26              |
| neovim_glsl.zeno_evaluation_after_mac  | require neovim_glsl.stage.after_mac.evaluation includes zeno | src/platform.rs and evaluation/route-v0.3/manifest.json                        |
| neovim_glsl.zeno_evaluation_observable | require neovim_glsl.zeno.evaluation = observable         | src/platform.rs emits a machine-readable report via --platform-report             |
| neovim_glsl.multi_target_portability_direction | require neovim_glsl.portability.direction = multi_target | src/platform.rs names more than one target                                |
| neovim_glsl.not_single_target_only     | forbid neovim_glsl.portability.scope = single_target_only | same; the report would be pointless for a single target                          |
| neovim_glsl.root_ui_integration_evaluation | require neovim_glsl.root_ui.integration.evaluation = hypothesis | src/root_ui.rs is a projection, not an adoption; evaluation/evidence/merged-root-ui-projection.json |

## domain pin — v0.4 note モデルと編集面 (18)

| pin id                                 | statement                                               | honored at                                                                        |
| -------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------- |
| neovim_glsl.primary_object             | require neovim_glsl.primary_object = markdown_note       | witness pending - this repository still opens files only                           |
| neovim_glsl.file_not_primary           | forbid neovim_glsl.primary_object = file                 | witness pending - and currently *violated in spirit* by the candidate, which is why it is a candidate |
| neovim_glsl.file_retained_for_programming | require neovim_glsl.file_model.retained_scope includes programming_task | the candidate edits files, which is the retained scope             |
| neovim_glsl.user_file_awareness_not_required | require neovim_glsl.user_facing.file_awareness = not_required | witness pending - no note surface exists yet                          |
| neovim_glsl.note_substrate             | require neovim_glsl.note.substrate = yui_notes           | witness pending - no note substrate is wired                                       |
| neovim_glsl.note_substrate_not_new     | forbid neovim_glsl.note.substrate = new_independent_store | witnessed by absence - nothing here creates a note store                          |
| neovim_glsl.storage_model              | require neovim_glsl.storage.model = local_repository_and_db | witness pending - open_question storage sync model is unresolved                |
| neovim_glsl.editing_mode_wysiwyg       | require neovim_glsl.editing_mode includes wysiwyg        | witness pending - the candidate has no WYSIWYG mode                                |
| neovim_glsl.editing_mode_vim           | require neovim_glsl.editing_mode includes vim            | the candidate is vim mode; Neovim keymaps reach it unmodified                       |
| neovim_glsl.editing_mode_surface_mobile | require neovim_glsl.surface.mobile.editing_mode = both  | witness pending - no mobile surface                                                |
| neovim_glsl.editing_mode_surface_web   | require neovim_glsl.surface.web.editing_mode = both      | witness pending - no web surface                                                   |
| neovim_glsl.root_ui_surface_adoption   | require neovim_glsl.root_ui.adoption.scope = editing_surface | witness pending - src/root_ui.rs projects, it does not implement the surface   |
| neovim_glsl.root_ui_replaces_current_surface | require neovim_glsl.editing_surface.implementation = root_ui | witness pending - blocked behind root_ui_hardening_order            |
| neovim_glsl.root_ui_hardening_order    | require neovim_glsl.root_ui.hardening.order = before_surface_replacement | honored by not having replaced the surface; open_question root_ui_hardening_done |
| neovim_glsl.capability_target          | require neovim_glsl.editor.capability_target = ide_level | witness pending - quarantined as unmeasurable; no threshold exists                 |
| neovim_glsl.capability_provider_own    | require neovim_glsl.editor.capability_provider = self_built | spec v0.6 editor_basis_own_host; evaluation/protocol-surface measures the surface to build |
| neovim_glsl.keymap_preservation        | require neovim_glsl.keymap.baseline = current_keymap     | evaluation/state-ownership drives the owner's own mappings unmodified               |
| neovim_glsl.keymap_no_redesign         | forbid neovim_glsl.keymap.baseline = redesigned          | witnessed by absence - nothing here defines a new keymap                            |

## domain pin — v0.4 委譲と後段 (4)

| pin id                                 | statement                                               | honored at                                                                        |
| -------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------- |
| neovim_glsl.delegated_editing          | require neovim_glsl.delegated_editing = required         | witness pending - no agent editing path exists in this repository                  |
| neovim_glsl.delegated_editing_agent    | require neovim_glsl.delegated_editing.agent = yui        | witness pending - same                                                             |
| neovim_glsl.lab_integration            | require neovim_glsl.integration.lab_minamorl_com = required | witness pending - deliberately after the editor surface                        |
| neovim_glsl.lab_integration_order      | require neovim_glsl.integration.lab_minamorl_com.order = after_editor_surface | honored by absence - nothing here connects to lab yet          |

## v0.5 資産保全 pin (2)

| pin id                                 | statement                                               | honored at                                                                        |
| -------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------- |
| neovim_glsl.neovim_asset_not_discarded | forbid neovim_glsl.neovim_assets.disposition = discarded | evaluation/ retains every Neovim measurement; nothing was deleted at v0.6          |
| neovim_glsl.neovim_retention_mode      | require neovim_glsl.neovim.retention_mode = editing_experience_preservation | keymap and protocol are what is kept, not the implementation    |

## property と example — v0.2 / v0.4 (5 + 8)

| id                                  | kind     | statement                                                          | honored at                                                    |
| ----------------------------------- | -------- | ------------------------------------------------------------------ | ------------------------------------------------------------- |
| neovim_glsl.aish_counterpart_witness | property | forall stage . integration_counterpart(stage) == ai_native_shell   | src/aish.rs integrates with aish and nothing else               |
| neovim_glsl.aish_repository_visibility_witness | property | forall snapshot . counterpart_repository_visibility(snapshot) == private | outside this repository; aish stays private           |
| neovim_glsl.primary_object_witness  | property | forall surface . primary_object(surface) == markdown_note           | witness pending - no note surface exists yet                   |
| neovim_glsl.editing_mode_witness    | property | forall surface in [mobile, web] . editing_modes(surface) == [wysiwyg, vim] | witness pending - neither surface exists              |
| neovim_glsl.keymap_witness          | property | forall motion in [h, j, k, l] . keymap_binding(motion) == current_keymap_binding(motion) | the candidate forwards keys to Neovim unchanged |
| neovim_glsl.aish_connection         | example  | integration_counterpart_choice => ai_native_shell                  | src/aish.rs                                                    |
| neovim_glsl.aish_integration_start  | example  | v0_2_instruction_deliverable => commenced_aish_integration         | src/aish.rs as the deliverable                                 |
| neovim_glsl.aish_repository_visibility | example | counterpart_repository_visibility_at_integration_start => private | outside this repository                                        |
| neovim_glsl.note_first              | example  | default_opened_object => markdown_note                             | witness pending - the candidate opens files                    |
| neovim_glsl.file_in_programming     | example  | programming_task_object => file                                    | the candidate edits source files, which is the retained scope   |
| neovim_glsl.mobile_modes            | example  | mobile_surface_editing_modes => wysiwyg_and_vim                     | witness pending - no mobile surface                            |
| neovim_glsl.surface_impl            | example  | editing_surface_implementation => root_ui                          | witness pending - src/root_ui.rs only projects                  |
| neovim_glsl.delegation              | example  | user_requests_edit_from_agent => yui_performs_edit                  | witness pending - no delegation path                           |

## retired / resolved v0.6 non-pin entries

| id                                      | kind          | status        | trace                                                                 |
| --------------------------------------- | ------------- | ------------- | --------------------------------------------------------------------- |
| neovim_glsl.editor_basis                | free          | RETIRED@0.6   | lifted into neovim_glsl.editor_basis_own_host                         |
| neovim_glsl.architecture                | quarantine    | RETIRED@0.6   | resolved by gate answer own_host_protocol                             |
| neovim_glsl.architecture_decision       | open_question | RESOLVED@0.6  | resolved by gate answer own_host_protocol                             |
| neovim_glsl.basis_selection             | open_question | RESOLVED@0.6  | resolved by gate answer own_host_protocol                             |
| neovim_glsl.neovim_asset_reuse_scope    | open_question | NARROWED@0.6  | protocol is pinned; remaining scope stays live in UNDECIDED.md        |

## retired / resolved v0.7 non-pin entries

| id                                        | kind          | status        | trace                                                             |
| ----------------------------------------- | ------------- | ------------- | ----------------------------------------------------------------- |
| neovim_glsl.telescope_customization       | free          | RETIRED@0.7   | depended on the telescope naming; successor is navigation_customization |
| neovim_glsl.telescope_under_own_host      | quarantine    | RETIRED@0.7   | the ambiguity vanished with the naming it was about              |
| neovim_glsl.telescope_realization_decision | open_question | RESOLVED@0.7  | answered as "neither" - the mechanism itself is no longer required |

## retired / resolved v0.8 non-pin entries

| id                                     | kind          | status        | trace                                                             |
| -------------------------------------- | ------------- | ------------- | ----------------------------------------------------------------- |
| neovim_glsl.navigation_ui_locus        | free          | RETIRED@0.8   | lifted into navigation_locus_choice by gate answer glsl_overlay   |
| neovim_glsl.external_surface_boundary  | quarantine    | RETIRED@0.8   | the boundary the question asked about is the answer               |
| neovim_glsl.navigation_surface_decision | open_question | RESOLVED@0.8 | resolved by gate answer glsl_overlay                              |

## v0.9 non-pin state change

| id                                  | kind          | status                    | trace                                                       |
| ----------------------------------- | ------------- | ------------------------- | ----------------------------------------------------------- |
| neovim_glsl.navigation_state_owner  | open_question | AWAITING_OBSERVATION@0.9  | gate answered unknown; evaluation/state-ownership measures both arrangements without choosing |

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
| quarantine    | 26    | not implemented; verbatim | UNDECIDED.md section quarantine; spec-mirror/neovim-glsl-0.7.lines                 |
| open_question | 30    | not implemented; verbatim | UNDECIDED.md section open_question; spec-mirror/neovim-glsl-0.7.lines              |
| free          | 42    | not locked; verbatim      | DESIGN-SPACE.md, generated from spec-mirror/neovim-glsl-0.7.lines                  |

これらの数は手で数えたものではない。`tools/sync_undecided.py` と
`tools/sync_design_space.py` が spec から数える。DESIGN-SPACE.md は v0.6 まで
`@0.1` の 11 軸を手書きしたまま腐っており、実際には 42 軸あった。UNDECIDED.md が
同じ腐り方をしていたのと同型の失敗なので、同じ直し方をした。

未決を実装しないこと自体が `impl.defer_open` の遵守であり、free を固定しないこと自体が `impl.free_not_locked` の遵守である。この repository に GLSL 化範囲・性能基準・shader・build 設定が無いのは欠落ではない。architecture は v0.6 で `own_host_speaking_neovim_protocol` に決まったが、protocol surface、transport、editing core、Lua runtime、telescope の実現形、embed candidate の去就はまだ固定しない。

`evaluation/free-surface/` も同様にこの行を変えない。あそこにあるのは
「grid の外に置くとこれが表現できる」という観測であって、置くべきだという判断ではない。
`open_question neovim_glsl.navigation_surface_decision` と
`quarantine neovim_glsl.external_surface_boundary` は依然として開いている。

`evaluation/` 配下の性能測定はこの行を変えない。あそこにあるのは観測であって基準ではなく、
`quarantine neovim_glsl.performance_criteria` と `free neovim_glsl.performance.numeric_targets`
は依然として空である。閾値は実行時に渡されたときだけ report に入り、既定では
`unset_awaiting_human_gate` と明記されて出る。

## 正本

`spec` が真実。この表と spec が食い違ったら spec が勝つ。表と repository を直し、pin を緩めない (`impl.spec_is_truth`)。
