package io.github.tom039224.yhtye.shared.update

import io.github.tom039224.yhtye.shared.version.AppVersion
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// GitHub Releases に出した Android 版の一覧から、自分より新しい版を選ぶ。アプリの外へ出る通信はこの確認だけ
// (docs/DEVELOPMENT.md「Android アプリのリリース」)。

/** リリースの一覧を読む GitHub の API (認証なし。1 時間に 60 回まで)。 */
const val RELEASES_URL: String = "https://api.github.com/repos/Tom039224/yhtye/releases?per_page=100"

/** リリースのページ。タグを足すとそのリリース (APK を添付している) のページになる。 */
const val RELEASE_PAGE_URL: String = "https://github.com/Tom039224/yhtye/releases/tag/"

/** Android 版のタグの頭 (`android-v0.1.0`)。デスクトップ版のリリースと分ける。 */
const val ANDROID_TAG_PREFIX: String = "android-v"

/** GitHub の `GET /repos/{owner}/{repo}/releases` の 1 件のうち、使う項目。 */
@Serializable
data class GithubRelease(
    @SerialName("tag_name") val tagName: String,
    val draft: Boolean = false,
    val prerelease: Boolean = false,
)

/** 自分より新しい Android 版のリリース。 */
data class AvailableUpdate(val version: AppVersion, val tag: String) {
    /** ブラウザで開くリリースのページ。URL はタグ (読み込むときに版の形を確かめた) から作り、API の応答の URL は使わない。 */
    val pageUrl: String get() = RELEASE_PAGE_URL + tag
}

/** [tag] が `android-v<版>` ならその更新の情報 (他のタグ・版として読めないタグは null)。 */
fun androidUpdateOfTag(tag: String): AvailableUpdate? {
    if (!tag.startsWith(ANDROID_TAG_PREFIX)) return null
    val text = tag.removePrefix(ANDROID_TAG_PREFIX)
    // URL に使うので、空白などの混ざったタグは受け取らない
    if (text != text.trim()) return null
    return AppVersion.parse(text)?.let { AvailableUpdate(it, tag) }
}

/**
 * [releases] のうち、[current] より新しい Android 版で一番新しいもの (無ければ null)。下書きは見ない。
 * プレリリースの印の付いたリリースも対象 (0.x の間は全部そう)。版の比べ方は Semantic Versioning。
 */
fun newestAndroidUpdate(releases: List<GithubRelease>, current: AppVersion): AvailableUpdate? = releases
    .filter { !it.draft }
    .mapNotNull { androidUpdateOfTag(it.tagName) }
    .filter { it.version > current }
    .maxByOrNull { it.version }
