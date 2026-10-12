package io.github.tom039224.yhtye.background

import android.content.Context
import android.content.Intent
import io.github.tom039224.yhtye.MainActivity

/** 通知のタップで開くチャット。[serial] は同じチャットをもう一度頼まれても気づくための番号。 */
data class OpenChatRequest(val project: String, val chat: String, val serial: Long)

/** 完了の通知のタップ (`PendingIntent`) が持つ Intent と、それを読む側。 */
object OpenChatIntent {
    const val ACTION = "io.github.tom039224.yhtye.OPEN_CHAT"
    private const val EXTRA_PROJECT = "project"
    private const val EXTRA_CHAT = "chat"

    /** [project] の [chat] を開く Intent。アクティビティは `singleTop` なので前面にあれば `onNewIntent` に届く。 */
    fun create(context: Context, project: String, chat: String): Intent =
        Intent(context, MainActivity::class.java)
            .setAction(ACTION)
            .putExtra(EXTRA_PROJECT, project)
            .putExtra(EXTRA_CHAT, chat)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)

    /** [intent] が完了の通知のタップなら (プロジェクトの ID, チャットの ID)。 */
    fun parse(intent: Intent?): Pair<String, String>? {
        if (intent?.action != ACTION) return null
        val project = intent.getStringExtra(EXTRA_PROJECT)?.takeIf { it.isNotEmpty() } ?: return null
        val chat = intent.getStringExtra(EXTRA_CHAT)?.takeIf { it.isNotEmpty() } ?: return null
        return project to chat
    }
}
