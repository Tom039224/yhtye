package io.github.tom039224.yhtye.shared.sync

import java.io.File

/** PC のテストと同じ記録 (`src/test/fixtures/<name>`) をリポジトリから読む。 */
actual fun readFixture(name: String): String {
    var dir: File? = File(System.getProperty("user.dir").orEmpty()).absoluteFile
    while (dir != null) {
        val file = File(dir, "src/test/fixtures/$name")
        if (file.isFile && File(dir, "mobile").isDirectory) return file.readText()
        dir = dir.parentFile
    }
    error("src/test/fixtures/$name was not found above ${System.getProperty("user.dir")}")
}
