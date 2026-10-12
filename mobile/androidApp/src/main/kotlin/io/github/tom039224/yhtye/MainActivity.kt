package io.github.tom039224.yhtye

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import io.github.tom039224.yhtye.background.OpenChatIntent
import io.github.tom039224.yhtye.ui.YhtyeRoot

class MainActivity : ComponentActivity() {
    private val session get() = (application as YhtyeApplication).session
    private val updates get() = (application as YhtyeApplication).updates

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        // 作り直し (回転など) のときは同じ Intent をもう一度受け取らない
        if (savedInstanceState == null) receive(intent)
        setContent {
            YhtyeTheme {
                YhtyeRoot(session, updates)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        receive(intent)
    }

    /**
     * `yhtye://pair?...` (QR をカメラ以外で読んだとき・adb での確認) をペアリング画面に、完了の通知のタップをそのチャットを
     * 開く頼みとして Session に渡す。
     */
    private fun receive(intent: Intent?) {
        OpenChatIntent.parse(intent)?.let { (project, chat) -> session.requestOpenChat(project, chat) }
        if (intent?.action != Intent.ACTION_VIEW) return
        intent.dataString?.let(session::receiveLink)
    }
}
