package io.github.tom039224.yhtye.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.Ring
import io.github.tom039224.yhtye.shared.sync.projectRing
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/**
 * ホーム: プロジェクト一覧とそれぞれのリング。同期層 ([AppStore]) の状態から作る (つながるたび・他の端末が
 * プロジェクトを開いたとき・作業の開始と終了で同期層が一覧を読み直す)。
 */
class HomeViewModel(private val sync: AppStore) : ViewModel() {
    data class Row(val project: ProjectInfo, val ring: Ring)

    data class Projects(
        /** まだ一度も取れていなければ null。 */
        val rows: List<Row>? = null,
        val refreshing: Boolean = false,
        /** 最後の失敗 (同期層のエラー)。 */
        val error: String? = null,
    )

    private val refreshing = MutableStateFlow(false)

    val projects: StateFlow<Projects> = combine(sync.state, refreshing) { state, busy ->
        val rows = state.projects
            .sortedBy { it.name.lowercase() }
            .map { Row(it, projectRing(it, state.project)) }
        Projects(
            rows = rows.takeIf { state.projectsLoaded },
            refreshing = busy,
            error = state.errors.lastOrNull()?.message,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(STOP_TIMEOUT_MS), Projects())

    /** 引っ張って更新。 */
    fun refresh() {
        if (refreshing.value) return
        refreshing.value = true
        viewModelScope.launch {
            try {
                sync.refreshProjects()
            } finally {
                refreshing.value = false
            }
        }
    }

    private companion object {
        const val STOP_TIMEOUT_MS = 5_000L
    }
}
