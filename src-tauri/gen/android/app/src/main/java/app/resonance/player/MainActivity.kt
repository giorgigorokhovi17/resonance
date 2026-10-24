package app.resonance.player

import android.content.ComponentName
import android.graphics.Color
import android.os.Bundle
import android.view.View
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture

class MainActivity : TauriActivity() {
  private var controller: ListenableFuture<MediaController>? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    // Light icons on the app's dark background (the default theme's --c-bg, src/lib/theme.ts).
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
    window.decorView.setBackgroundColor(Color.parseColor("#07080b"))
    // Android 15 draws apps under the status and navigation bars; keep the page clear of them.
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
    NativeBridge.activityCreated()
  }

  override fun onStart() {
    super.onStart()
    // Binding starts the playback service; it turns itself into a foreground service while
    // music plays, so playback outlives this screen.
    val token = SessionToken(this, ComponentName(this, PlaybackService::class.java))
    controller = MediaController.Builder(this, token).buildAsync()
  }

  override fun onStop() {
    controller?.let { MediaController.releaseFuture(it) }
    controller = null
    super.onStop()
  }
}
