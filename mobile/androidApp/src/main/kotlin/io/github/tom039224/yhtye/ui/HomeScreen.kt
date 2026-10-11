package io.github.tom039224.yhtye.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import io.github.tom039224.yhtye.R
import io.github.tom039224.yhtye.shared.api.generated.ProjectInfo
import io.github.tom039224.yhtye.shared.client.ConnectionState
import io.github.tom039224.yhtye.shared.client.YhtyeClient
import io.github.tom039224.yhtye.shared.credentials.Credentials
import io.github.tom039224.yhtye.shared.sync.AppStore
import io.github.tom039224.yhtye.shared.sync.RING_LABEL
import io.github.tom039224.yhtye.shared.sync.Ring

/** ペアリング後: 接続状態とプロジェクト一覧。 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HomeScreen(credentials: Credentials, client: YhtyeClient, sync: AppStore, onUnpair: () -> Unit) {
    val model: HomeViewModel = viewModel(key = credentials.deviceId) { HomeViewModel(sync) }
    val state by client.state.collectAsStateWithLifecycle()
    val projects by model.projects.collectAsStateWithLifecycle()
    var confirmUnpair by rememberSaveable { mutableStateOf(false) }

    Scaffold(
        modifier = Modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.app_name)) },
                actions = { HomeMenu(onUnpair = { confirmUnpair = true }) },
            )
        },
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = projects.refreshing,
            onRefresh = model::refresh,
            modifier = Modifier.fillMaxSize().padding(padding),
        ) {
            LazyColumn(
                modifier = Modifier.fillMaxSize().testTag("projects"),
                contentPadding = PaddingValues(vertical = 8.dp),
            ) {
                item { ConnectionCard(state, credentials, onReconnect = client::start) }
                item {
                    Text(
                        stringResource(R.string.home_projects),
                        style = MaterialTheme.typography.titleMedium,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                    )
                }
                projects.error?.let { error ->
                    item {
                        Text(
                            stringResource(R.string.home_load_failed, error),
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.padding(horizontal = 16.dp),
                        )
                    }
                }
                val list = projects.rows
                if (list != null && list.isEmpty()) {
                    item {
                        Text(
                            stringResource(R.string.home_no_projects),
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.padding(horizontal = 16.dp),
                        )
                    }
                }
                items(list.orEmpty(), key = { it.project.id }) { row ->
                    ProjectRow(row.project, row.ring)
                    HorizontalDivider()
                }
            }
        }
    }

    if (confirmUnpair) {
        AlertDialog(
            onDismissRequest = { confirmUnpair = false },
            title = { Text(stringResource(R.string.unpair_title)) },
            text = { Text(stringResource(R.string.unpair_message)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmUnpair = false
                        onUnpair()
                    },
                    modifier = Modifier.testTag("unpairConfirm"),
                ) { Text(stringResource(R.string.home_unpair)) }
            },
            dismissButton = {
                TextButton(onClick = { confirmUnpair = false }) { Text(stringResource(R.string.cancel)) }
            },
        )
    }
}

@Composable
private fun HomeMenu(onUnpair: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    val menu = stringResource(R.string.home_menu)
    Box {
        IconButton(
            onClick = { open = true },
            modifier = Modifier.testTag("menu").semantics { contentDescription = menu },
        ) {
            Text("⋮", style = MaterialTheme.typography.titleLarge)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text(stringResource(R.string.home_unpair)) },
                onClick = {
                    open = false
                    onUnpair()
                },
                modifier = Modifier.testTag("unpair"),
            )
        }
    }
}

@Composable
private fun ConnectionCard(state: ConnectionState, credentials: Credentials, onReconnect: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(Modifier.size(10.dp).background(stateColor(state), CircleShape))
                Text(
                    connectionStateText(state),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.testTag("connectionState"),
                )
            }
            Text(
                "${credentials.hosts.first()}:${credentials.port}",
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(stringResource(R.string.home_device, credentials.deviceName), style = MaterialTheme.typography.bodySmall)
            if (state is ConnectionState.WaitingToReconnect && state.reason != null) {
                Text(stringResource(R.string.state_waiting_reason, state.reason.orEmpty()), style = MaterialTheme.typography.bodySmall)
            }
            when (state) {
                is ConnectionState.WaitingToReconnect ->
                    TextButton(onClick = onReconnect) { Text(stringResource(R.string.state_reconnect_now)) }
                is ConnectionState.Rejected ->
                    TextButton(onClick = onReconnect) { Text(stringResource(R.string.retry)) }
                else -> Unit
            }
        }
    }
}

@Composable
private fun stateColor(state: ConnectionState): Color = when (state) {
    is ConnectionState.Connected -> CONNECTED_GREEN
    is ConnectionState.Connecting, is ConnectionState.WaitingToReconnect -> MaterialTheme.colorScheme.tertiary
    ConnectionState.Idle -> MaterialTheme.colorScheme.outline
    ConnectionState.Revoked, is ConnectionState.Rejected -> MaterialTheme.colorScheme.error
}

private val CONNECTED_GREEN = Color(0xFF2E7D32)

@Composable
private fun ProjectRow(project: ProjectInfo, ring: Ring) {
    val status = when {
        ring == Ring.Running -> stringResource(R.string.home_project_running)
        project.open -> stringResource(R.string.home_project_open)
        else -> null
    }
    ListItem(
        leadingContent = { ProjectRing(ring) },
        headlineContent = { Text(project.name, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        supportingContent = { Text(project.path, maxLines = 2, overflow = TextOverflow.Ellipsis) },
        trailingContent = status?.let { { Text(it, style = MaterialTheme.typography.labelMedium) } },
        modifier = Modifier.testTag("project-${project.id}"),
    )
}

/** PC のプロジェクトのリング (T-49): 作業が動いていれば回る緑、なければ中を抜いた緑。 */
@Composable
private fun ProjectRing(ring: Ring) {
    val label = RING_LABEL.getValue(ring)
    val modifier = Modifier.size(RING_SIZE).semantics { contentDescription = label }.testTag("ring-${ring.name}")
    if (ring == Ring.Running) {
        CircularProgressIndicator(modifier = modifier, color = CONNECTED_GREEN, strokeWidth = RING_STROKE)
    } else {
        Box(modifier.border(RING_STROKE, CONNECTED_GREEN, CircleShape))
    }
}

private val RING_SIZE = 16.dp
private val RING_STROKE = 2.dp
