package io.github.tom039224.yhtye.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.shared.pairing.InvalidPairingInput
import io.github.tom039224.yhtye.shared.pairing.PairingFailure

// 共通モジュールの理由・状態を画面の文言にする。

@Composable
fun invalidInputText(reason: InvalidPairingInput): String = when (reason) {
    InvalidPairingInput.NotPairingUri -> stringResource(R.string.input_not_pairing_uri)
    is InvalidPairingInput.UnsupportedVersion -> stringResource(R.string.input_unsupported_version, reason.version)
    InvalidPairingInput.MissingHost -> stringResource(R.string.input_missing_host)
    is InvalidPairingInput.NotTailscaleHost -> stringResource(R.string.input_not_tailscale, reason.host)
    InvalidPairingInput.InvalidPort -> stringResource(R.string.input_invalid_port)
    InvalidPairingInput.MissingCode -> stringResource(R.string.input_missing_code)
    InvalidPairingInput.InvalidCode -> stringResource(R.string.input_invalid_code)
}

/** 失敗の文言と、PC が返した説明 (あれば)。 */
@Composable
fun pairingFailureText(failure: PairingFailure): Pair<String, String?> = when (failure) {
    is PairingFailure.CodeRejected -> stringResource(R.string.fail_code_rejected) to failure.message
    is PairingFailure.OtherUser -> stringResource(R.string.fail_other_user) to failure.message
    is PairingFailure.BadRequest -> stringResource(R.string.fail_bad_request) to failure.message
    is PairingFailure.Unreachable -> stringResource(
        R.string.fail_unreachable,
        failure.hosts.joinToString(", ") { "$it:${failure.port}" },
    ) to failure.cause
    is PairingFailure.WrongServer -> stringResource(R.string.fail_wrong_server) to null
    is PairingFailure.ServerError -> stringResource(R.string.fail_server_error, failure.status) to failure.message
}
