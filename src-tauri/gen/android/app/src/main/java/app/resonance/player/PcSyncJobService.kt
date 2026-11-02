package app.resonance.player

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Environment
import android.provider.MediaStore
import java.io.File

/**
 * User-initiated data transfer job (docs/phone-file-sync.md) keeping the process alive while the
 * Rust sync with the PC runs. The PC is on the home network, so it asks for Wi-Fi, not internet.
 */
class PcSyncJobService : JobService() {
  override fun onCreate() {
    super.onCreate()
    NativeBridge.appContext = applicationContext
    NativeBridge.register()
    ensureChannel(this)
  }

  override fun onStartJob(params: JobParameters): Boolean {
    setNotification(params, NOTIF_ID, buildNotification(this), JOB_END_NOTIFICATION_POLICY_REMOVE)
    return running
  }

  override fun onStopJob(params: JobParameters): Boolean = false

  companion object {
    private const val JOB_ID = 4403
    private const val NOTIF_ID = 4404
    private const val CHANNEL_ID = "pc_sync"
    private const val SYNC_DIR = "Music/Undertone/Synced"

    @Volatile private var running = false

    fun onRunning(context: Context, nowRunning: Boolean) {
      running = nowRunning
      val appCtx = context.applicationContext
      val scheduler = appCtx.getSystemService(JobScheduler::class.java) ?: return
      if (!nowRunning) {
        runCatching { scheduler.cancel(JOB_ID) }
        return
      }
      val wifi = NetworkRequest.Builder()
        .removeCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
        .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
        .build()
      val job = JobInfo.Builder(JOB_ID, ComponentName(appCtx, PcSyncJobService::class.java))
        .setUserInitiated(true)
        .setRequiredNetwork(wifi)
        .build()
      runCatching { scheduler.schedule(job) }
    }

    /** Private storage for songs still downloading. */
    fun stagingDir(context: Context): String {
      val base = context.getExternalFilesDir(null) ?: context.filesDir
      return File(base, "pc-sync").apply { mkdirs() }.absolutePath
    }

    /** Moves a finished song into `Music/Undertone/Synced`, at `path` under it. */
    fun publish(context: Context, srcPath: String, path: String): String {
      val folder = path.substringBeforeLast('/', "")
      val dir = if (folder.isEmpty()) SYNC_DIR else "$SYNC_DIR/$folder"
      return Soulseek.publishAudio(context, srcPath, dir, path.substringAfterLast('/'))
    }

    /** Deletes a song this app synced; true once it's gone. */
    fun remove(context: Context, path: String): Boolean = runCatching {
      val selection = "${MediaStore.MediaColumns.DATA} = ?"
      context.contentResolver.delete(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, selection, arrayOf(path))
      !File(path).exists()
    }.getOrDefault(false)

    private fun ensureChannel(context: Context) {
      val nm = context.getSystemService(NotificationManager::class.java) ?: return
      if (nm.getNotificationChannel(CHANNEL_ID) != null) return
      val ch = NotificationChannel(CHANNEL_ID, "Sync with PC", NotificationManager.IMPORTANCE_LOW).apply {
        description = "Shows while songs sync with your PC."
      }
      nm.createNotificationChannel(ch)
    }

    private fun buildNotification(context: Context): Notification {
      ensureChannel(context)
      val open = Intent(context, MainActivity::class.java).apply {
        flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
      }
      val pi = PendingIntent.getActivity(context, 1, open, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
      return Notification.Builder(context, CHANNEL_ID)
        .setSmallIcon(R.mipmap.ic_launcher)
        .setContentTitle("Undertone")
        .setContentText("Syncing with your PC…")
        .setContentIntent(pi)
        .setOngoing(true)
        .build()
    }
  }
}
