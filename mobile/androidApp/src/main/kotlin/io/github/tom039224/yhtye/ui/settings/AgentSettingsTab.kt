package io.github.tom039224.yhtye.ui.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.tom039224.yhtye.MonoFont
import io.github.tom039224.yhtye.Yhtye
import io.github.tom039224.yhtye.shared.api.generated.AgentRole
import io.github.tom039224.yhtye.shared.api.generated.AgentSettingsView
import io.github.tom039224.yhtye.shared.api.generated.Candidate
import io.github.tom039224.yhtye.shared.api.generated.HarnessModels
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.api.generated.RoleSettings
import io.github.tom039224.yhtye.shared.sync.AgentSettingsController
import io.github.tom039224.yhtye.shared.sync.AgentSettingsState
import io.github.tom039224.yhtye.shared.sync.Edit
import io.github.tom039224.yhtye.shared.sync.FirstModel
import io.github.tom039224.yhtye.shared.sync.MAX_NOTE_CHARS
import io.github.tom039224.yhtye.shared.sync.ROLES
import io.github.tom039224.yhtye.shared.sync.RoleEntry
import io.github.tom039224.yhtye.shared.sync.SettingsCommands
import io.github.tom039224.yhtye.shared.sync.addRow
import io.github.tom039224.yhtye.shared.sync.choiceLabel
import io.github.tom039224.yhtye.shared.sync.effortText
import io.github.tom039224.yhtye.shared.sync.firstModel
import io.github.tom039224.yhtye.shared.sync.isLongList
import io.github.tom039224.yhtye.shared.sync.isMissing
import io.github.tom039224.yhtye.shared.sync.knownEfforts
import io.github.tom039224.yhtye.shared.sync.modelKey
import io.github.tom039224.yhtye.shared.sync.modelText
import io.github.tom039224.yhtye.shared.sync.newRow
import io.github.tom039224.yhtye.shared.sync.of
import io.github.tom039224.yhtye.shared.sync.pickerModels
import io.github.tom039224.yhtye.shared.sync.removeRow
import io.github.tom039224.yhtye.shared.sync.rowChoice
import io.github.tom039224.yhtye.shared.sync.sameChoice
import io.github.tom039224.yhtye.shared.sync.updateRow
import io.github.tom039224.yhtye.shared.sync.withDefaultRow
import io.github.tom039224.yhtye.ui.common.YIcon
import io.github.tom039224.yhtye.ui.common.YIcons

const val AGENT_NOTE =
    "役割ごとに、使えるハーネス × モデル × effort の行を並べます。★ の行が既定で、オーケストレータは各行の用途メモを読んで" +
        "タスクごとに行を選べます (行と完全に一致するものだけ)。変更はすぐ保存され、新しく起動するエージェントから有効です。"

private const val ADD_FAILED = "追加できるハーネス × モデルがありません (すべて使われているか、一覧を取得できていません)"

private fun roleIcon(role: AgentRole): ImageVector = when (role) {
    AgentRole.Orchestrator -> YIcons.Message
    AgentRole.Implementer -> YIcons.Code
    AgentRole.Investigator -> YIcons.Search
    AgentRole.Reviewer -> YIcons.Eye
    AgentRole.Unrecognized -> YIcons.Bot
}

/**
 * 「エージェント」の節 (PC の AgentSettingsSection): 役割ごとの候補の行 (ハーネス × モデル × effort + 用途メモ) と既定の行を、
 * 全体かこのプロジェクトの層で。変更はすぐ保存する (表示している版を付けて。他の端末が先に変えていたら保存せず知らせる)。
 * 他の端末の変更は読み直し、入力中ならバナー。
 */
@Composable
fun AgentSettingsTab(commands: SettingsCommands, project: ProjectInfo?) {
    val scope = rememberCoroutineScope()
    val controller = remember(commands) { AgentSettingsController(commands, scope) }
    var projectScope by rememberSaveable { mutableStateOf(project != null) }
    val layer = if (projectScope && project != null) project.id else null
    LaunchedEffect(controller, layer) { controller.showLayer(layer) }
    val state by controller.state.collectAsState()
    val stale by controller.stale.collectAsState()
    val view = state.view
    SettingsPane("agent-section", stale, controller::reloadNow) {
        ScopeToggle(project, projectScope && project != null) { projectScope = it }
        Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            NoteText(AGENT_NOTE, Modifier.weight(1f))
            IconAction(
                YIcons.Refresh,
                "モデル一覧を再取得",
                onClick = controller::refreshModels,
                enabled = view != null && !state.loadingModels,
                spin = state.loadingModels,
                tag = "refresh-models",
            )
        }
        view?.harnesses?.forEach { h ->
            val m = state.models[h.id]
            when {
                m == null -> Unit
                m.loading -> StatusLine("${h.label}: モデル一覧を取得中…")
                m.error != null -> StatusLine("${h.label}: モデル一覧を取得できません — ${m.error}", error = true)
                m.models?.models?.isEmpty() == true ->
                    if (h.requiresModel) {
                        StatusLine("${h.label}: モデル一覧が空のため選べません", error = true)
                    } else {
                        StatusLine("${h.label}: モデルを選べないハーネスです (既定のモデルを使います)")
                    }
            }
        }
        if (view != null && view.harnesses.isEmpty()) {
            WarningLine(
                "使えるハーネスが見つかりません。設定 › ハーネス を開いて、検出状態の確認や実行ファイルのパスの指定をしてください。",
                tag = "no-harness",
            )
        }
        state.error?.let { StatusLine(it, error = true, tag = "agent-error") }
        if (view == null) {
            if (state.error == null) StatusLine("読み込み中…", tag = "agent-loading")
        } else {
            ROLES.forEach { entry -> RoleCard(entry, view, projectScope && project != null, state, controller) }
        }
    }
}

/** 設定の範囲: 全体 (globe) と このプロジェクト (folder)。プロジェクトを開いていなければ後者は押せない。 */
@Composable
private fun ScopeToggle(project: ProjectInfo?, projectSelected: Boolean, onSelect: (Boolean) -> Unit) {
    val c = Yhtye.colors
    val shape = RoundedCornerShape(18.dp)
    Row(Modifier.clip(shape).border(1.dp, c.lineCard, shape).semantics { contentDescription = "設定の範囲" }.testTag("scope-toggle")) {
        ScopeButton(YIcons.Globe, "全体", !projectSelected, true, "scope-global") { onSelect(false) }
        ScopeButton(YIcons.Folder, project?.name ?: "プロジェクトを開いていません", projectSelected, project != null, "scope-project") { onSelect(true) }
    }
}

@Composable
private fun ScopeButton(icon: ImageVector, text: String, on: Boolean, enabled: Boolean, tag: String, onClick: () -> Unit) {
    val c = Yhtye.colors
    val fg = when {
        on -> c.accent
        enabled -> c.tx3
        else -> c.tx5
    }
    Row(
        Modifier
            .heightIn(min = 40.dp)
            .background(if (on) c.accentSoft else Yhtye.colors.bgCenter)
            .clickable(enabled = enabled, role = Role.Tab, onClick = onClick)
            .padding(horizontal = 12.dp)
            .semantics { selected = on }
            .testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        YIcon(icon, fg, 14.dp)
        Text(text, color = fg, fontSize = 13.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun RoleCard(
    entry: RoleEntry,
    view: AgentSettingsView,
    projectScope: Boolean,
    state: AgentSettingsState,
    controller: AgentSettingsController,
) {
    val c = Yhtye.colors
    val role = entry.role
    val layer = if (projectScope) view.projectLayer else view.global
    val own = layer.of(role)
    val shown = view.effective.of(role)
    val inherits = own == null
    val locked = state.saving || (projectScope && inherits)
    var expanded by rememberSaveable(role.wire) { mutableIntStateOf(-1) }
    val builtin = choiceLabel(view.builtin, view.harnesses, state.models[view.builtin.harness]?.models)
    val missing = shown.candidates.filter { isMissing(it, view.harnesses) }.map { it.harness }.distinct()
    SettingsCard(Modifier.semantics { contentDescription = entry.label }.testTag("role-${role.wire}")) {
        Row(
            Modifier.fillMaxWidth().background(c.bgInset).padding(horizontal = 12.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            YIcon(roleIcon(role), c.tx2, 16.dp)
            Text(entry.label, color = c.tx, fontSize = 14.sp)
            Text(entry.hint, color = c.tx4, fontSize = 12.sp)
        }
        InheritRow(
            if (projectScope) "全体の設定を使う" else "組み込みの既定を使う",
            checked = inherits,
            enabled = !state.saving,
            tag = "inherit-${role.wire}",
        ) { checked -> controller.save(role, if (checked) null else shown) }
        if (!projectScope && inherits) {
            NoteText("組み込みの既定: $builtin", Modifier.padding(horizontal = 12.dp).padding(bottom = 6.dp))
        }
        if (missing.isNotEmpty()) {
            WarningLine(
                "${missing.joinToString(" / ")} が見つかりません (インストールされていない)。該当する候補は使えず、代わりに既定 ($builtin) で起動します。",
                Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                tag = "missing-${role.wire}",
            )
        }
        shown.candidates.forEachIndexed { i, row ->
            HorizontalDivider(color = c.line)
            CandidateRow(
                role = role,
                label = entry.label,
                index = i,
                row = row,
                settings = shown,
                view = view,
                state = state,
                locked = locked,
                expanded = expanded == i,
                onToggle = { expanded = if (expanded == i) -1 else i },
                controller = controller,
            )
        }
        HorizontalDivider(color = c.line)
        val add = {
            val row = newRow(view.harnesses, state.models, shown)
            if (row == null) {
                controller.fail(ADD_FAILED)
            } else {
                val edit = addRow(shown, row)
                if (edit is Edit.Ok) expanded = shown.candidates.size
                controller.apply(role, edit)
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 44.dp)
                .clickable(enabled = !locked, role = Role.Button, onClick = add)
                .padding(horizontal = 12.dp)
                .testTag("add-row-${role.wire}"),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            YIcon(YIcons.Plus, if (locked) c.tx5 else c.tx3, 14.dp)
            Text("行を追加", color = if (locked) c.tx5 else c.tx3, fontSize = 13.sp)
        }
    }
}

@Composable
private fun InheritRow(text: String, checked: Boolean, enabled: Boolean, tag: String, onChange: (Boolean) -> Unit) {
    val c = Yhtye.colors
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 44.dp)
            .clickable(enabled = enabled, role = Role.Checkbox) { onChange(!checked) }
            .padding(horizontal = 4.dp)
            .testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(
            checked = checked,
            onCheckedChange = null,
            enabled = enabled,
            colors = CheckboxDefaults.colors(checkedColor = c.accent, uncheckedColor = c.tx4, checkmarkColor = c.onAccent),
            modifier = Modifier.padding(horizontal = 8.dp),
        )
        Text(text, color = c.tx2, fontSize = 13.sp)
    }
}

/** 候補の 1 行: ★ (既定にする)・ハーネス × モデル × effort・削除と、押すと開く編集 (ハーネス・モデル・effort・用途メモ)。 */
@Composable
private fun CandidateRow(
    role: AgentRole,
    label: String,
    index: Int,
    row: Candidate,
    settings: RoleSettings,
    view: AgentSettingsView,
    state: AgentSettingsState,
    locked: Boolean,
    expanded: Boolean,
    onToggle: () -> Unit,
    controller: AgentSettingsController,
) {
    val c = Yhtye.colors
    val n = index + 1
    val key = "${role.wire}-$index"
    val isDefault = sameChoice(rowChoice(row), settings.default)
    val missing = isMissing(row, view.harnesses)
    val listed = state.models[row.harness]?.models
    Column(Modifier.fillMaxWidth().testTag("row-$key")) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconAction(
                YIcons.Star,
                "$label 候補$n を既定にする",
                onClick = { controller.save(role, withDefaultRow(settings, index)) },
                enabled = !locked && !isDefault,
                tint = if (isDefault) c.accent else c.tx5,
                tag = "default-$key",
                modifier = Modifier.semantics { selected = isDefault },
            )
            Column(
                Modifier
                    .weight(1f)
                    .heightIn(min = 44.dp)
                    .clickable(enabled = !locked, role = Role.Button, onClick = onToggle)
                    .padding(vertical = 8.dp)
                    .semantics { contentDescription = "$label 候補$n" }
                    .testTag("summary-$key"),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(
                    choiceLabel(rowChoice(row), view.harnesses, listed) + if (missing) " (見つからない)" else "",
                    color = if (missing) c.handFg else c.tx,
                    fontFamily = MonoFont,
                    fontSize = 12.5.sp,
                    lineHeight = 18.sp,
                )
                if (row.note.isNotEmpty()) {
                    Text(row.note, color = c.tx3, fontSize = 12.5.sp, lineHeight = 18.sp)
                } else {
                    Text("用途メモ (例: 設計が絡む難しい変更に)", color = c.tx5, fontSize = 12.5.sp)
                }
            }
            YIcon(if (expanded) YIcons.ChevronDown else YIcons.ChevronRight, c.tx4, 14.dp, Modifier.padding(horizontal = 4.dp))
            IconAction(
                YIcons.Trash,
                "$label 候補$n を削除",
                onClick = { removeRow(settings, index)?.let { controller.save(role, it) } },
                enabled = !locked && settings.candidates.size > 1,
                tag = "remove-$key",
            )
        }
        if (expanded && !locked) {
            RowEditor(role, label, index, row, settings, view, state, listed, controller)
        }
    }
}

@Composable
private fun RowEditor(
    role: AgentRole,
    label: String,
    index: Int,
    row: Candidate,
    settings: RoleSettings,
    view: AgentSettingsView,
    state: AgentSettingsState,
    listed: HarnessModels?,
    controller: AgentSettingsController,
) {
    val n = index + 1
    val key = "${role.wire}-$index"
    val missing = isMissing(row, view.harnesses)
    val onEdit = { edit: Edit -> controller.apply(role, edit) }
    Column(Modifier.fillMaxWidth().padding(start = 14.dp, end = 14.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        FieldLabel("ハーネス")
        val harnessChoices = buildList {
            if (missing) add(Choice(row.harness, "${row.harness} (見つからない)"))
            view.harnesses.forEach { h ->
                add(Choice(h.id, h.label, enabled = h.id == row.harness || firstModel(h, state.models[h.id]?.models) != FirstModel.NoneAvailable))
            }
        }
        SelectField(
            shown = view.harnesses.find { it.id == row.harness }?.label ?: "${row.harness} (見つからない)",
            choices = harnessChoices,
            onSelect = { id ->
                val h = view.harnesses.find { it.id == id }
                val first = h?.let { firstModel(it, state.models[id]?.models) }
                if (first is FirstModel.Of) onEdit(updateRow(settings, index) { it.copy(harness = id, model = first.model, effort = null) })
            },
            label = "$label 候補$n のハーネス",
            tag = "harness-$key",
        )
        FieldLabel("モデル")
        ModelCell("$label 候補$n のモデル", "model-$key", row, listed, enabled = !missing) { model ->
            val keep = knownEfforts(listed, state.efforts, row.harness, model)?.any { it.value == row.effort } ?: false
            onEdit(updateRow(settings, index) { it.copy(model = model, effort = if (keep) it.effort else null) })
        }
        FieldLabel("effort")
        EffortCell("$label 候補$n の effort", "effort-$key", row, listed, state, enabled = !missing) { effort ->
            onEdit(updateRow(settings, index) { it.copy(effort = effort) })
        }
        FieldLabel("用途メモ (オーケストレータが行を選ぶ手がかり)")
        NoteField(row.note, "$label 候補$n の用途メモ", "note-$key", controller, key) { note ->
            onEdit(updateRow(settings, index) { it.copy(note = note) })
        }
    }
}

@Composable
private fun FieldLabel(text: String) {
    Text(text, color = Yhtye.colors.tx4, fontSize = 11.5.sp, modifier = Modifier.padding(top = 2.dp))
}

/** 用途メモ: 打っている間はここで持ち、フォーカスが外れるか完了で保存する (変わっていれば)。 */
@Composable
private fun NoteField(saved: String, label: String, tag: String, controller: AgentSettingsController, key: String, onCommit: (String) -> Unit) {
    var text by remember(saved) { mutableStateOf(saved) }
    val commit = { if (text != saved) onCommit(text) }
    SettingsField(
        value = text,
        onChange = { text = it },
        placeholder = "例: 設計が絡む難しい変更に",
        label = label,
        tag = tag,
        mono = false,
        maxLength = MAX_NOTE_CHARS,
        onDone = commit,
        onFocus = { focused ->
            controller.editing.set("note-$key", focused)
            if (!focused) commit()
        },
    )
}

@Composable
private fun ModelCell(label: String, tag: String, row: Candidate, listed: HarnessModels?, enabled: Boolean, onChange: (String) -> Unit) {
    when {
        listed == null || listed.models.isEmpty() ->
            Text(row.model ?: "既定のモデル", color = Yhtye.colors.tx2, fontFamily = MonoFont, fontSize = 13.sp, modifier = Modifier.testTag(tag))
        isLongList(listed) -> ModelPicker(label, tag, row.model, listed, enabled, onChange)
        else -> {
            val current = row.model
            val unlisted = current != null && listed.models.none { it.value == current }
            val choices = buildList {
                if (current == null) add(Choice("", "既定のモデル"))
                if (unlisted) add(Choice(current, "$current (一覧に無い)"))
                listed.models.forEach { add(Choice(it.value, modelText(it.value, listed))) }
            }
            SelectField(
                shown = current?.let { if (unlisted) "$it (一覧に無い)" else modelText(it, listed) } ?: "既定のモデル",
                choices = choices,
                onSelect = onChange,
                label = label,
                tag = tag,
                enabled = enabled,
            )
        }
    }
}

/** モデルが多いハーネス (OpenCode: 何百) の選び方: 押すと検索欄が開き、一致を提供元ごとに出す。 */
@Composable
private fun ModelPicker(label: String, tag: String, value: String?, listed: HarnessModels, enabled: Boolean, onChange: (String) -> Unit) {
    val c = Yhtye.colors
    var open by remember { mutableStateOf(false) }
    var query by remember { mutableStateOf("") }
    val result = pickerModels(listed, query)
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 40.dp)
                .clip(FIELD_SHAPE)
                .border(1.dp, c.lineCard, FIELD_SHAPE)
                .background(c.bgInset)
                .clickable(enabled = enabled, role = Role.DropdownList) { open = !open }
                .padding(horizontal = 10.dp)
                .semantics { contentDescription = label }
                .testTag(tag),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                value?.let { modelText(it, listed) } ?: "既定のモデル",
                color = if (enabled) c.tx else c.tx4,
                fontFamily = MonoFont,
                fontSize = 13.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            YIcon(YIcons.ChevronDown, c.tx4, 14.dp)
        }
        if (open) {
            SettingsField(
                value = query,
                onChange = { query = it },
                placeholder = "${listed.models.size} モデルから検索 (例: free, gpt)",
                label = "${label}を検索",
                tag = "$tag-search",
            )
            if (query.isBlank()) StatusLine("検索語を入力してください (空白区切りで AND)")
            if (query.isNotBlank() && result.groups.isEmpty()) StatusLine("一致するモデルがありません")
            result.groups.forEach { g ->
                if (result.groups.size > 1) Text(g.provider ?: "その他", color = c.tx4, fontSize = 11.5.sp, modifier = Modifier.padding(top = 4.dp))
                g.models.forEach { m ->
                    Text(
                        modelText(m.value, listed),
                        color = if (m.value == value) c.accent else c.tx,
                        fontFamily = MonoFont,
                        fontSize = 13.sp,
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = 40.dp)
                            .clickable(role = Role.Button) {
                                open = false
                                query = ""
                                onChange(m.value)
                            }
                            .padding(horizontal = 10.dp, vertical = 10.dp)
                            .testTag("$tag-option-${m.value}"),
                    )
                }
            }
            if (result.more > 0) StatusLine("他に ${result.more} 件 — 検索語を絞ってください")
        }
    }
}

@Composable
private fun EffortCell(
    label: String,
    tag: String,
    row: Candidate,
    listed: HarnessModels?,
    state: AgentSettingsState,
    enabled: Boolean,
    onChange: (String?) -> Unit,
) {
    val known = knownEfforts(listed, state.efforts, row.harness, row.model)
    val current = row.effort
    if (known == null || known.isEmpty()) {
        // まだ分からない・読めなかった・effort の無いモデル: 今の値を見せるだけ
        val efforts = row.model?.let { state.efforts[modelKey(row.harness, it)] }
        val hint = when {
            known != null -> "このモデルに effort はありません"
            efforts?.error != null -> "effort を取得できません — ${efforts.error}"
            else -> "effort を読み込み中…"
        }
        SelectField(shown = current ?: "指定なし", choices = emptyList(), onSelect = {}, label = label, tag = tag, enabled = false, hint = hint)
        return
    }
    val unlisted = current != null && known.none { it.value == current }
    val choices = buildList {
        add(Choice("", "指定なし"))
        if (unlisted) add(Choice(current, "$current (一覧に無い)"))
        known.forEach { add(Choice(it.value, effortText(it))) }
    }
    SelectField(
        shown = when {
            current == null -> "指定なし"
            unlisted -> "$current (一覧に無い)"
            else -> effortText(known.first { it.value == current })
        },
        choices = choices,
        onSelect = { onChange(it.ifEmpty { null }) },
        label = label,
        tag = tag,
        enabled = enabled,
    )
}
