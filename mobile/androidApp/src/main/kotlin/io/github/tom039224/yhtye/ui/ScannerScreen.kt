package io.github.tom039224.yhtye.ui

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.ImageAnalysis
import androidx.camera.mlkit.vision.MlKitAnalyzer
import androidx.camera.view.LifecycleCameraController
import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.shared.pairing.InvalidPairingInput
import io.github.tom039224.yhtye.shared.pairing.InvalidPairingInputException
import io.github.tom039224.yhtye.shared.pairing.PairingTarget
import io.github.tom039224.yhtye.shared.pairing.looksLikePairingUri
import io.github.tom039224.yhtye.shared.pairing.parsePairingUri

/** PC の QR コード (`yhtye://pair?...`) をカメラで読む。カメラが使えなければ手入力へ案内する。 */
@Composable
fun ScannerScreen(onScanned: (PairingTarget) -> Unit, onManual: () -> Unit, onBack: () -> Unit) {
    val context = LocalContext.current
    fun hasCamera() = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
    var granted by remember { mutableStateOf(hasCamera()) }
    var denied by rememberSaveable { mutableStateOf(false) }
    val request = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        granted = ok
        denied = !ok
    }
    LaunchedEffect(Unit) {
        if (!granted && !denied) request.launch(Manifest.permission.CAMERA)
    }
    // 設定画面で許可して戻ってきたとき
    LifecycleResumeEffect(Unit) {
        granted = hasCamera()
        onPauseOrDispose {}
    }

    if (!granted) {
        PairingScaffold(title = stringResource(R.string.scan_title), onBack = onBack) {
            Text(
                stringResource(if (denied) R.string.camera_permission_denied else R.string.camera_permission_needed),
                style = MaterialTheme.typography.bodyLarge,
            )
            Button(onClick = { request.launch(Manifest.permission.CAMERA) }, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.camera_permission_request))
            }
            if (denied) {
                OutlinedButton(
                    onClick = {
                        val uri = Uri.fromParts("package", context.packageName, null)
                        context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, uri))
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) { Text(stringResource(R.string.open_app_settings)) }
            }
            OutlinedButton(onClick = onManual, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.pairing_manual))
            }
        }
        return
    }

    var hint by remember { mutableStateOf<String?>(null) }
    var cameraError by remember { mutableStateOf<String?>(null) }
    var done by remember { mutableStateOf(false) }
    val notYhtye = stringResource(R.string.scan_not_yhtye)
    val messages = rememberScanMessages()
    Box(Modifier.fillMaxSize().background(Color.Black)) {
        QrCameraPreview(
            onText = { text ->
                if (done) return@QrCameraPreview
                if (!looksLikePairingUri(text)) {
                    hint = notYhtye
                    return@QrCameraPreview
                }
                parsePairingUri(text).fold(
                    onSuccess = {
                        done = true
                        onScanned(it)
                    },
                    onFailure = { hint = messages((it as InvalidPairingInputException).reason) },
                )
            },
            onError = { cameraError = it },
        )
        Surface(
            color = Color.Black.copy(alpha = 0.6f),
            contentColor = Color.White,
            modifier = Modifier.align(Alignment.BottomCenter).fillMaxWidth(),
        ) {
            Column(
                modifier = Modifier.safeDrawingPadding().padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(stringResource(R.string.scan_hint), style = MaterialTheme.typography.bodyLarge)
                (cameraError?.let { stringResource(R.string.camera_unavailable, it) } ?: hint)?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.errorContainer)
                }
                OutlinedButton(onClick = onManual, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.pairing_manual), color = Color.White)
                }
                TextButton(onClick = onBack, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.back), color = Color.White)
                }
            }
        }
    }
}

@Composable
private fun rememberScanMessages(): (InvalidPairingInput) -> String {
    val unsupported = stringResource(R.string.input_unsupported_version)
    val other = stringResource(R.string.scan_not_yhtye)
    val notTailscale = stringResource(R.string.input_not_tailscale)
    return { reason ->
        when (reason) {
            is InvalidPairingInput.UnsupportedVersion -> unsupported.format(reason.version)
            is InvalidPairingInput.NotTailscaleHost -> notTailscale.format(reason.host)
            else -> other
        }
    }
}

/** カメラの映像と、映った QR コードの文字列 ([onText]。同じものが何度も来る)。 */
@Composable
private fun QrCameraPreview(onText: (String) -> Unit, onError: (String) -> Unit) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val currentOnText by rememberUpdatedState(onText)
    val currentOnError by rememberUpdatedState(onError)
    val controller = remember { LifecycleCameraController(context) }
    DisposableEffect(lifecycleOwner) {
        val executor = ContextCompat.getMainExecutor(context)
        val scanner = BarcodeScanning.getClient(
            BarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build(),
        )
        controller.setImageAnalysisAnalyzer(
            executor,
            MlKitAnalyzer(listOf(scanner), ImageAnalysis.COORDINATE_SYSTEM_ORIGINAL, executor) { result ->
                result.getValue(scanner)?.firstNotNullOfOrNull { it.rawValue }?.let { currentOnText(it) }
            },
        )
        try {
            controller.bindToLifecycle(lifecycleOwner)
        } catch (e: IllegalStateException) {
            currentOnError(e.message ?: e::class.java.simpleName)
        } catch (e: IllegalArgumentException) { // カメラが無い
            currentOnError(e.message ?: e::class.java.simpleName)
        }
        onDispose {
            controller.unbind()
            controller.clearImageAnalysisAnalyzer()
            scanner.close()
        }
    }
    AndroidView(
        factory = { PreviewView(it).apply { this.controller = controller } },
        modifier = Modifier.fillMaxSize(),
    )
}
