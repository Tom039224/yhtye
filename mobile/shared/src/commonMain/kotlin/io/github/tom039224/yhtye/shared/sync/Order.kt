package io.github.tom039224.yhtye.shared.sync

// プロジェクトとブランチの「最近使った順」。リスト (新しく使った順) はコアが持ち (`crates/yhtye-core/src/order.rs`)、
// プロジェクトの一覧はコアが並べて返す。ブランチの木は読んだリストで自分のノードを並べる。ここは PC の
// `src/store/order.ts` の移植: その並べ替えと、メニューの「上へ / 下へ」が作るリスト。

/** [items] を [order] の中の [keyOf] の位置の順に。載っていないものはその後に、渡された順のまま。 */
fun <T> sortByOrder(items: List<T>, order: List<String>, keyOf: (T) -> String): List<T> {
    val rank = HashMap<String, Int>()
    order.forEachIndexed { i, key -> rank.putIfAbsent(key, i) }
    // `sortedBy` は安定: 載っていないものは元の順のまま
    return items.sortedBy { rank[keyOf(it)] ?: Int.MAX_VALUE }
}

/** [keys] の中で [key] を 1 つ上 (`-1`) か下 (`1`) に動かしたもの。その向きには動かせなければ null。 */
fun step(keys: List<String>, key: String, delta: Int): List<String>? {
    val from = keys.indexOf(key)
    val to = from + delta
    if (from < 0 || to < 0 || to >= keys.size) return null
    return keys.toMutableList().also { it[from] = keys[to]; it[to] = keys[from] }
}

/**
 * 全体のリスト [full] のうち、画面に出ている [shown] だけをユーザーが並べ替えた後の全体: [shown] のキーは [full] で
 * 持っていた場所のまま新しい順に入り、ほかのキーは動かない。一部だけ出ている一覧 (バックグラウンドのチップ) で使う。
 */
fun fillSlots(full: List<String>, shown: List<String>): List<String> {
    val among = shown.toSet()
    val queue = ArrayDeque(shown)
    return full.map { key -> if (key in among) (queue.removeFirstOrNull() ?: key) else key }
}

/** コアが返したブランチの順に、前に足した分 ([keys]) を先頭にして残りを続けたもの (コアが保存するのと同じ)。 */
fun mergeOrder(keys: List<String>, rest: List<String>): List<String> = keys + rest.filter { it !in keys }
