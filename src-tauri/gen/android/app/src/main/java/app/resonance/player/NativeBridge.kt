package app.resonance.player

import android.os.Handler
import android.os.Looper
import android.os.SystemClock

/**
 * The Rust audio engine, reached through JNI (src-tauri/src/mobile/smtc.rs). Rust pushes the
 * current song and play state here; the media session reads them and sends commands back.
 */
object NativeBridge {
  init {
    System.loadLibrary("resonance_lib")
  }

  const val STOPPED = 0
  const val PLAYING = 1
  const val PAUSED = 2

  /** Lets Rust find this class from its own threads, and resends the current state. */
  @JvmStatic external fun register()

  /** MainActivity created; rebuilds the page if the previous screen took it along. */
  @JvmStatic external fun activityCreated()

  /** "play", "pause", "toggle", "next", "previous", "stop", or "seek" to `value` seconds. */
  @JvmStatic external fun command(name: String, value: Double)

  fun command(name: String) = command(name, 0.0)

  class Track(val title: String, val artist: String, val album: String, val cover: String?, val durationMs: Long)

  @Volatile var track: Track? = null
    private set
  @Volatile var state = STOPPED
    private set
  /** Position at `positionAt` (elapsedRealtime), so it can be extrapolated while playing. */
  @Volatile var positionMs = 0L
    private set
  @Volatile var positionAt = 0L
    private set

  /** Called on the main thread after every change. */
  var listener: (() -> Unit)? = null

  private val main = Handler(Looper.getMainLooper())

  @JvmStatic
  fun onTrack(title: String, artist: String, album: String, cover: String?, duration: Double) {
    track = Track(title, artist, album, cover, (duration * 1000).toLong())
    main.post { listener?.invoke() }
  }

  @JvmStatic
  fun onPlayback(newState: Int, position: Double) {
    state = newState
    positionMs = (position * 1000).toLong()
    positionAt = SystemClock.elapsedRealtime()
    main.post { listener?.invoke() }
  }
}
