package io.github.tom039224.yhtye.ui

import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.session.Session
import io.github.tom039224.yhtye.shared.pairing.DEFAULT_PORT
import io.github.tom039224.yhtye.shared.pairing.InvalidPairingInput
import io.github.tom039224.yhtye.shared.pairing.InvalidPairingInputException
import io.github.tom039224.yhtye.shared.pairing.PairingFailure
import io.github.tom039224.yhtye.shared.pairing.PairingResult
import io.github.tom039224.yhtye.shared.pairing.PairingTarget
import io.github.tom039224.yhtye.shared.pairing.normalizeDeviceName
import io.github.tom039224.yhtye.shared.pairing.parseManualEntry
import kotlinx.coroutines.launch

private enum class Step { Start, Scan, Manual, Confirm }

/** 回転などで画面が作り直されても、読み取った接続先を失わない。 */
private val TargetSaver = listSaver<PairingTarget?, String>(
    save = { t -> if (t == null) emptyList() else listOf(t.port.toString(), t.code, t.serverId.orEmpty()) + t.hosts },
    restore = { v ->
        if (v.size < 4) null else PairingTarget(v.drop(3), v[0].toInt(), v[1], v[2].ifEmpty { null })
    },
)

/** ペアリング前: 始め → (QR を読む | 手入力) → 端末名の確認 → ペアリング。 */
@Composable
fun PairingFlow(session: Session, revoked: Boolean, incoming: Session.Incoming?) {
    var step by rememberSaveable { mutableStateOf(Step.Start) }
    var target by rememberSaveable(stateSaver = TargetSaver) { mutableStateOf<PairingTarget?>(null) }
    var linkError by rememberSaveable { mutableStateOf<String?>(null) }
    val invalidLink = (incoming as? Session.Incoming.Invalid)?.let { invalidInputText(it.reason) }

    LaunchedEffect(incoming) {
        when (incoming) {
            is Session.Incoming.Target -> {
                target = incoming.target
                step = Step.Confirm
                linkError = null
            }
            is Session.Incoming.Invalid -> {
                step = Step.Start
                linkError = invalidLink
            }
            null -> return@LaunchedEffect
        }
        session.consumeIncoming()
    }
    BackHandler(enabled = step != Step.Start) { step = Step.Start }

    when (step) {
        Step.Start -> PairingStart(
            revoked = revoked,
            linkError = linkError,
            onScan = {
                session.dismissRevoked()
                step = Step.Scan
            },
            onManual = {
                session.dismissRevoked()
                step = Step.Manual
            },
        )
        Step.Scan -> ScannerScreen(
            onScanned = {
                target = it
                step = Step.Confirm
            },
            onManual = { step = Step.Manual },
            onBack = { step = Step.Start },
        )
        Step.Manual -> ManualEntry(
            onNext = {
                target = it
                step = Step.Confirm
            },
            onBack = { step = Step.Start },
        )
        Step.Confirm -> {
            val current = target
            if (current == null) {
                LaunchedEffect(Unit) { step = Step.Start }
            } else {
                ConfirmPairing(session, current, onBack = { step = Step.Start })
            }
        }
    }
}

@Composable
private fun PairingStart(revoked: Boolean, linkError: String?, onScan: () -> Unit, onManual: () -> Unit) {
    PairingScaffold(title = stringResource(R.string.pairing_title)) {
        if (revoked) Notice(stringResource(R.string.pairing_revoked))
        if (linkError != null) Notice(stringResource(R.string.pairing_link_invalid, linkError))
        Text(stringResource(R.string.pairing_intro), style = MaterialTheme.typography.bodyLarge)
        Spacer(Modifier.height(8.dp))
        Button(onClick = onScan, modifier = Modifier.fillMaxWidth().testTag("scan")) {
            Text(stringResource(R.string.pairing_scan))
        }
        OutlinedButton(onClick = onManual, modifier = Modifier.fillMaxWidth().testTag("manual")) {
            Text(stringResource(R.string.pairing_manual))
        }
    }
}

@Composable
private fun ManualEntry(onNext: (PairingTarget) -> Unit, onBack: () -> Unit) {
    var host by rememberSaveable { mutableStateOf("") }
    var port by rememberSaveable { mutableStateOf(DEFAULT_PORT.toString()) }
    var code by rememberSaveable { mutableStateOf("") }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    var errorField by rememberSaveable { mutableStateOf<Field?>(null) }
    val messages = rememberInputMessages()

    fun submit() {
        parseManualEntry(host, port, code).fold(
            onSuccess = onNext,
            onFailure = {
                val reason = (it as InvalidPairingInputException).reason
                errorField = fieldOf(reason)
                error = messages.text(reason)
            },
        )
    }

    PairingScaffold(title = stringResource(R.string.manual_title), onBack = onBack) {
        Text(stringResource(R.string.manual_intro), style = MaterialTheme.typography.bodyLarge)
        OutlinedTextField(
            value = host,
            onValueChange = { host = it },
            label = { Text(stringResource(R.string.manual_host)) },
            placeholder = { Text(stringResource(R.string.manual_host_placeholder)) },
            singleLine = true,
            isError = errorField == Field.Host,
            supportingText = error.takeIf { errorField == Field.Host }?.let { { Text(it) } },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next, autoCorrectEnabled = false),
            modifier = Modifier.fillMaxWidth().testTag("host"),
        )
        OutlinedTextField(
            value = port,
            onValueChange = { port = it.filter(Char::isDigit).take(5) },
            label = { Text(stringResource(R.string.manual_port)) },
            singleLine = true,
            isError = errorField == Field.Port,
            supportingText = error.takeIf { errorField == Field.Port }?.let { { Text(it) } },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number, imeAction = ImeAction.Next),
            modifier = Modifier.fillMaxWidth().testTag("port"),
        )
        OutlinedTextField(
            value = code,
            onValueChange = { code = it.uppercase().take(MAX_CODE_INPUT) },
            label = { Text(stringResource(R.string.manual_code)) },
            placeholder = { Text(stringResource(R.string.manual_code_placeholder)) },
            singleLine = true,
            isError = errorField == Field.Code,
            supportingText = error.takeIf { errorField == Field.Code }?.let { { Text(it) } },
            keyboardOptions = KeyboardOptions(
                capitalization = KeyboardCapitalization.Characters,
                keyboardType = KeyboardType.Password,
                imeAction = ImeAction.Done,
                autoCorrectEnabled = false,
            ),
            keyboardActions = KeyboardActions(onDone = { submit() }),
            modifier = Modifier.fillMaxWidth().testTag("code"),
        )
        Button(onClick = ::submit, modifier = Modifier.fillMaxWidth().testTag("next")) {
            Text(stringResource(R.string.next))
        }
    }
}

/** 区切り (`-` や空白) を含めて打てる長さ。 */
private const val MAX_CODE_INPUT = 12

private enum class Field { Host, Port, Code }

private fun fieldOf(reason: InvalidPairingInput): Field? = when (reason) {
    InvalidPairingInput.MissingHost, is InvalidPairingInput.NotTailscaleHost -> Field.Host
    InvalidPairingInput.InvalidPort -> Field.Port
    InvalidPairingInput.MissingCode, InvalidPairingInput.InvalidCode -> Field.Code
    InvalidPairingInput.NotPairingUri, is InvalidPairingInput.UnsupportedVersion -> null
}

/** コールバックの中 (Composable の外) で文言を作るため、今の Context の文言を先に取っておく。 */
@Composable
private fun rememberInputMessages(): InputMessages = InputMessages(
    missingHost = stringResource(R.string.input_missing_host),
    notTailscale = stringResource(R.string.input_not_tailscale),
    invalidPort = stringResource(R.string.input_invalid_port),
    missingCode = stringResource(R.string.input_missing_code),
    invalidCode = stringResource(R.string.input_invalid_code),
    notPairingUri = stringResource(R.string.input_not_pairing_uri),
    unsupportedVersion = stringResource(R.string.input_unsupported_version),
)

private class InputMessages(
    private val missingHost: String,
    private val notTailscale: String,
    private val invalidPort: String,
    private val missingCode: String,
    private val invalidCode: String,
    private val notPairingUri: String,
    private val unsupportedVersion: String,
) {
    fun text(reason: InvalidPairingInput): String = when (reason) {
        InvalidPairingInput.MissingHost -> missingHost
        is InvalidPairingInput.NotTailscaleHost -> notTailscale.format(reason.host)
        InvalidPairingInput.InvalidPort -> invalidPort
        InvalidPairingInput.MissingCode -> missingCode
        InvalidPairingInput.InvalidCode -> invalidCode
        InvalidPairingInput.NotPairingUri -> notPairingUri
        is InvalidPairingInput.UnsupportedVersion -> unsupportedVersion.format(reason.version)
    }
}

@Composable
private fun ConfirmPairing(session: Session, target: PairingTarget, onBack: () -> Unit) {
    var name by rememberSaveable { mutableStateOf(Build.MODEL.orEmpty()) }
    var busy by rememberSaveable { mutableStateOf(false) }
    // 失敗の表示は画面の回転で消えてよい (もう一度押せばよい)
    var failure by remember { mutableStateOf<PairingFailure?>(null) }
    val scope = rememberCoroutineScope()
    val nameValid = normalizeDeviceName(name) != null

    PairingScaffold(title = stringResource(R.string.confirm_title), onBack = onBack.takeIf { !busy }) {
        Text(
            stringResource(R.string.confirm_target, target.hosts.first(), target.port),
            style = MaterialTheme.typography.bodyLarge,
        )
        OutlinedTextField(
            value = name,
            onValueChange = { name = it },
            label = { Text(stringResource(R.string.confirm_device_name)) },
            singleLine = true,
            enabled = !busy,
            isError = !nameValid,
            supportingText = if (nameValid) null else ({ Text(stringResource(R.string.confirm_device_name_invalid)) }),
            modifier = Modifier.fillMaxWidth().testTag("deviceName"),
        )
        failure?.let { f ->
            val (text, detail) = pairingFailureText(f)
            Notice(text, detail?.let { stringResource(R.string.fail_detail, it) })
        }
        Button(
            onClick = {
                busy = true
                failure = null
                scope.launch {
                    val result = session.pair(target, name)
                    // 成功したら Session がホームに切り替える
                    if (result is PairingResult.Failed) failure = result.failure
                    busy = false
                }
            },
            enabled = !busy && nameValid,
            modifier = Modifier.fillMaxWidth().testTag("pair"),
        ) {
            if (busy) {
                Row(horizontalArrangement = Arrangement.Center) {
                    CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
                    Spacer(Modifier.width(8.dp))
                }
            }
            Text(stringResource(R.string.confirm_pair))
        }
    }
}

/** ペアリングの各画面の枠: 上に題と「戻る」、本文は縦に並べてスクロール (キーボードを避ける)。 */
@Composable
fun PairingScaffold(title: String, onBack: (() -> Unit)? = null, content: @Composable ColumnScope.() -> Unit) {
    Scaffold(modifier = Modifier.fillMaxSize()) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .imePadding()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            if (onBack != null) {
                TextButton(onClick = onBack, modifier = Modifier.testTag("back")) { Text(stringResource(R.string.back)) }
            }
            Text(title, style = MaterialTheme.typography.headlineMedium)
            content()
        }
    }
}

/** 目立たせる注意書き。 */
@Composable
fun Notice(text: String, detail: String? = null) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.errorContainer,
            contentColor = MaterialTheme.colorScheme.onErrorContainer,
        ),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(text, style = MaterialTheme.typography.bodyMedium)
            if (detail != null) Text(detail, style = MaterialTheme.typography.bodySmall)
        }
    }
}
