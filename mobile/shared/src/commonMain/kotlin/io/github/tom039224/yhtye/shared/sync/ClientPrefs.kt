package io.github.tom039224.yhtye.shared.sync

import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json

// このクライアント (端末) だけの小さな設定: 最後に開いたプロジェクト、プロジェクトごとに表示していたチャット、
// チャットごとの下書き。保存の仕方は境界 ([ClientPrefs]) の向こう (Android は SharedPreferences)。
// PC の `src/store/prefs.ts` / `drafts.ts` に当たる。

/** 端末に残す文字列の設定。読めない・書けないときは覚えないだけ (例外を投げない)。 */
interface ClientPrefs {
    fun get(key: String): String?

    /** [value] を覚える。null で忘れる。 */
    fun set(key: String, value: String?)

    /** 全部忘れる (ペアリングを解除したとき。別の PC のプロジェクトや下書きを残さない)。 */
    fun clear()
}

/** メモリだけ (テスト・保存しない端末)。スレッド安全ではない (1 つのスレッドから使う)。 */
class MemoryPrefs(initial: Map<String, String> = emptyMap()) : ClientPrefs {
    private val values = HashMap(initial)

    override fun get(key: String): String? = values[key]

    override fun set(key: String, value: String?) {
        if (value == null) values.remove(key) else values[key] = value
    }

    override fun clear() {
        values.clear()
    }
}

/** 送っていないコンポーザの文。チャットごとに 1 つ (別のチャット・プロジェクトを出している間も残る)。 */
interface Drafts {
    /** チャットの下書き (無ければ `""`)。 */
    fun get(project: String, chat: String): String

    /** [text] を下書きとして覚える。空なら忘れる。 */
    fun set(project: String, chat: String, text: String)
}

/** [ClientPrefs] に下書きを残す (アプリが裏で殺されても残る)。 */
class PrefsDrafts(private val prefs: ClientPrefs) : Drafts {
    override fun get(project: String, chat: String): String = prefs.get(draftKey(project, chat)).orEmpty()

    override fun set(project: String, chat: String, text: String) {
        prefs.set(draftKey(project, chat), text.ifEmpty { null })
    }
}

/** 区切り文字を含む ID が混ざらない鍵 (`["a/b","c"]` と `["a","b/c"]` は別)。 */
private fun draftKey(project: String, chat: String): String =
    "yhtye.draft." + Json.encodeToString(ListSerializer(String.serializer()), listOf(project, chat))

/** 設定の鍵: 最後に開いたプロジェクトのパス (起動し直したら開き直す)。 */
const val LAST_PROJECT_KEY: String = "yhtye.lastProject"

/** 設定の鍵 (プロジェクトごと): プロジェクトを開き直したときに出すチャット。 */
fun chatPrefKey(project: String): String = "yhtye.chat.$project"
