package io.github.tom039224.yhtye.shared.api

import kotlinx.serialization.json.Json

/**
 * PC の Yhtye とやり取りする JSON の設定 (生成された API 型 `api.generated` と一緒に使う)。
 *
 * - `ignoreUnknownKeys`: PC 側 (新しい版) で増えたフィールドは無視する。
 *   知らない variant・enum 値は生成された型の `Unrecognized` になる (クラッシュしない)。
 * - `encodeDefaults` + `explicitNulls`: Rust と同じく、省略可能でないフィールドは null でも書く。
 *   Rust が省くフィールド (`skip_serializing_if`) は生成側で `@EncodeDefault(NEVER)` が付いている。
 */
val ApiJson: Json = Json {
    ignoreUnknownKeys = true
    encodeDefaults = true
    explicitNulls = true
}
