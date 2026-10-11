package io.github.tom039224.yhtye.background

import android.app.Application
import android.content.Intent
import androidx.test.core.app.ApplicationProvider
import io.github.tom039224.yhtye.shared.sync.BackgroundMode
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** 再起動・更新の後に常駐を再開するのは、ペアリング済みで「常に接続」のときだけ。 */
@RunWith(RobolectricTestRunner::class)
class BootReceiverTest {
    private val app: Application = ApplicationProvider.getApplicationContext()

    private fun settings(paired: Boolean, mode: BackgroundMode) {
        DeviceSettings.from(app).apply {
            this.paired = paired
            setMode(mode)
        }
    }

    private fun receive(action: String): Intent? {
        BootReceiver().onReceive(app, Intent(action))
        return shadowOf(app).nextStartedService
    }

    @Test
    fun `starts the service after a reboot when paired and always connected`() {
        settings(paired = true, mode = BackgroundMode.Always)
        assertEquals(ConnectionService::class.java.name, receive(Intent.ACTION_BOOT_COMPLETED)?.component?.className)
    }

    @Test
    fun `starts it again after the app was updated`() {
        settings(paired = true, mode = BackgroundMode.Always)
        assertEquals(ConnectionService::class.java.name, receive(Intent.ACTION_MY_PACKAGE_REPLACED)?.component?.className)
    }

    @Test
    fun `does not start it in the work only mode`() {
        settings(paired = true, mode = BackgroundMode.WorkOnly)
        assertNull(receive(Intent.ACTION_BOOT_COMPLETED))
    }

    @Test
    fun `does not start it when not paired`() {
        settings(paired = false, mode = BackgroundMode.Always)
        assertNull(receive(Intent.ACTION_BOOT_COMPLETED))
    }

    @Test
    fun `ignores other broadcasts`() {
        settings(paired = true, mode = BackgroundMode.Always)
        assertNull(receive(Intent.ACTION_TIME_CHANGED))
    }
}
