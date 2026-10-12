package io.github.tom039224.yhtye.background

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import io.github.tom039224.yhtye.MainActivity
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.shared.sync.BackgroundMode
import io.github.tom039224.yhtye.shared.sync.TurnNotifier
import io.github.tom039224.yhtye.shared.sync.UsersTurn
import io.github.tom039224.yhtye.shared.sync.usersTurnText
import io.github.tom039224.yhtye.ui.conversation.NEW_CHAT_TITLE

/** 通知のチャネル (端末の設定で個別に変えられる)。 */
object Channels {
    /** 常駐の常設の通知。静かに出す。 */
    const val CONNECTION = "connection"

    /** 作業の完了。ヘッドアップで出す。 */
    const val WORK_DONE = "work_done"

    /** 接続の異常 (PC から外された)。 */
    const val ALERT = "alert"
}

/** セッション ([io.github.tom039224.yhtye.session.Session]) が使う通知の窓口。 */
interface SessionNotifications : TurnNotifier {
    /** この端末が PC から外された (失効)。 */
    fun revoked()

    /** チャットの完了の通知を全部消す (ペアリングを解除したので)。 */
    fun cancelAll()
}

/**
 * Android の通知。完了の通知はチャットごとに 1 つ (同じチャットの次の完了は置き換える)、タップでそのプロジェクトのチャットを開く。
 * 通知が許可されていなければ何もしない (Android 13 以降は `POST_NOTIFICATIONS`)。
 */
class SystemNotifications(context: Context) : SessionNotifications {
    private val context = context.applicationContext
    private val manager = NotificationManagerCompat.from(this.context)

    /** 通知のチャネルを作る (何度呼んでもよい)。 */
    fun ensureChannels() {
        val system = context.getSystemService(NotificationManager::class.java)
        system.createNotificationChannel(
            NotificationChannel(Channels.CONNECTION, context.getString(R.string.notif_channel_connection), NotificationManager.IMPORTANCE_LOW)
                .apply { description = context.getString(R.string.notif_channel_connection_desc) },
        )
        system.createNotificationChannel(
            NotificationChannel(Channels.WORK_DONE, context.getString(R.string.notif_channel_done), NotificationManager.IMPORTANCE_HIGH)
                .apply { description = context.getString(R.string.notif_channel_done_desc) },
        )
        system.createNotificationChannel(
            NotificationChannel(Channels.ALERT, context.getString(R.string.notif_channel_alert), NotificationManager.IMPORTANCE_DEFAULT)
                .apply { description = context.getString(R.string.notif_channel_alert_desc) },
        )
    }

    /** 常駐の常設の通知。[connected] は PC につながっているか、[mode] は今の常駐方式。タップでアプリ。 */
    fun serviceNotification(connected: Boolean, mode: BackgroundMode): Notification =
        NotificationCompat.Builder(context, Channels.CONNECTION)
            .setSmallIcon(R.drawable.ic_stat_yhtye)
            .setContentTitle(context.getString(if (connected) R.string.notif_service_connected else R.string.notif_service_reconnecting))
            .setContentText(
                context.getString(if (mode == BackgroundMode.Always) R.string.notif_service_always else R.string.notif_service_work_only),
            )
            .setContentIntent(openApp())
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setShowWhen(false)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .build()

    override fun post(turn: UsersTurn) {
        val text = usersTurnText(turn, NEW_CHAT_TITLE)
        val tap = PendingIntent.getActivity(
            context,
            requestCode(turn.project, turn.chat),
            OpenChatIntent.create(context, turn.project, turn.chat),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val notification = NotificationCompat.Builder(context, Channels.WORK_DONE)
            .setSmallIcon(R.drawable.ic_stat_yhtye)
            .setContentTitle(text.title)
            .apply { text.body?.let { setContentText(it).setStyle(NotificationCompat.BigTextStyle().bigText(it)) } }
            .setContentIntent(tap)
            .setAutoCancel(true)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setWhen(turn.tsMs)
            .setShowWhen(true)
            .build()
        notify(turnTag(turn.project, turn.chat), TURN_ID, notification)
    }

    override fun cancel(project: String, chat: String) {
        manager.cancel(turnTag(project, chat), TURN_ID)
    }

    override fun cancelAll() {
        val system = context.getSystemService(NotificationManager::class.java)
        system.activeNotifications.filter { it.tag?.startsWith(TURN_TAG_PREFIX) == true }.forEach { manager.cancel(it.tag, it.id) }
    }

    override fun revoked() {
        val notification = NotificationCompat.Builder(context, Channels.ALERT)
            .setSmallIcon(R.drawable.ic_stat_yhtye)
            .setContentTitle(context.getString(R.string.notif_revoked_title))
            .setContentText(context.getString(R.string.notif_revoked_text))
            .setContentIntent(openApp())
            .setAutoCancel(true)
            .setCategory(NotificationCompat.CATEGORY_ERROR)
            .build()
        notify(REVOKED_TAG, REVOKED_ID, notification)
    }

    /** 通知が許されていれば出す。 */
    @SuppressLint("MissingPermission") // 下で POST_NOTIFICATIONS を確かめている
    private fun notify(tag: String, id: Int, notification: Notification) {
        if (!canPost()) return
        manager.notify(tag, id, notification)
    }

    private fun canPost(): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return false
        }
        return manager.areNotificationsEnabled()
    }

    private fun openApp(): PendingIntent = PendingIntent.getActivity(
        context,
        0,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    companion object {
        /** 常駐の常設の通知の ID。 */
        const val SERVICE_ID = 1
        private const val TURN_ID = 1
        private const val REVOKED_ID = 2
        private const val REVOKED_TAG = "revoked"
        private const val TURN_TAG_PREFIX = "turn/"

        /** チャットごとの通知の識別子 (チャットの ID は `C-<n>` で `/` を含まないので、プロジェクトの ID と混ざらない)。 */
        fun turnTag(project: String, chat: String): String = "$TURN_TAG_PREFIX$project/$chat"

        private fun requestCode(project: String, chat: String): Int = turnTag(project, chat).hashCode()
    }
}
