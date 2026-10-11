package io.github.tom039224.yhtye.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.tom039224.yhtye.shared.api.generated.ApiCommand
import io.github.tom039224.yhtye.shared.api.generated.ApiResponse
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.RpcException
import io.github.tom039224.yhtye.shared.client.YhtyeClient
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** ホーム: プロジェクト一覧。つながるたびに (再接続を含む) 取り直す。 */
class HomeViewModel(private val client: YhtyeClient) : ViewModel() {
    data class Projects(
        /** まだ一度も取れていなければ null。 */
        val projects: List<ProjectInfo>? = null,
        val refreshing: Boolean = false,
        val error: String? = null,
    )

    private val _projects = MutableStateFlow(Projects())
    val projects: StateFlow<Projects> = _projects.asStateFlow()

    private var loading: Job? = null

    init {
        viewModelScope.launch {
            // Connected の番号が変わる = つながり直した (切れていた間の変化を取り直す)
            client.state.collect { if (it is ConnectionState.Connected) refresh() }
        }
    }

    fun refresh() {
        loading?.cancel()
        loading = viewModelScope.launch {
            _projects.update { it.copy(refreshing = true) }
            val next = try {
                when (val response = client.call(ApiCommand.ListProjects, LIST_TIMEOUT_MS)) {
                    is ApiResponse.Projects -> Projects(projects = response.projects.sortedBy { it.name.lowercase() })
                    else -> _projects.value.copy(error = "unexpected answer: $response")
                }
            } catch (e: RpcException) {
                _projects.value.copy(error = e.message)
            }
            _projects.value = next.copy(refreshing = false)
        }
    }

    private companion object {
        const val LIST_TIMEOUT_MS = 15_000L
    }
}
