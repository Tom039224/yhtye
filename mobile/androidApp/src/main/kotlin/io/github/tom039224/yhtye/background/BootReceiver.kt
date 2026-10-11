package io.github.tom039224.yhtye.background

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import io.github.tom039224.yhtye.shared.sync.BackgroundMode

/**
 * 端末の再起動と、アプリの更新 (プロセスが落ちる) の後に、「常に接続」でペアリング済みなら常駐を始める。
 * この 2 つの受信機からは裏でも Foreground Service を始められる。種類が `specialUse` なので `BOOT_COMPLETED` からも
 * 始められる (`dataSync` は Android 15 以降できない)。ペアリングの有無はここでは手がかり ([DeviceSettings.paired]) だけで見て、
 * 本当にペアリングしているかは Session が決める (していなければサービスは自分で止まる)。
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        val device = DeviceSettings.from(context)
        if (device.paired && device.mode.value == BackgroundMode.Always) ConnectionService.start(context)
    }
}
