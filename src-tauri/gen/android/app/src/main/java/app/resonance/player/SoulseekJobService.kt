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
import android.os.Handler
import android.os.Looper

/**
 * User-initiated data transfer job (docs/android-design.md §5) keeping the process alive while
 * the Rust Soulseek download queue runs. Requires Wi-Fi (unmetered network) by default, unless the
 * user turns off "Download on Wi-Fi only".
 */
class SoulseekJobService : JobService() {
  private var activeParams: JobParameters? = null

  override fun onCreate() {
    super.onCreate()
    NativeBridge.appContext = applicationContext
    NativeBridge.register()
    ensureChannel(this)
  }

  override fun onStartJob(params: JobParameters): Boolean {
    instance = this
    activeParams = params
    val count = activeCount
    setNotification(
      params,
      NOTIF_ID,
      buildNotification(this, count.coerceAtLeast(1)),
      JOB_END_NOTIFICATION_POLICY_REMOVE,
    )
    if (count <= 0) {
      NativeBridge.setDownloadJobRunning(false)
      activeParams = null
      instance = null
      return false
    }
    NativeBridge.setDownloadJobRunning(true)
    return true
  }

  override fun onStopJob(params: JobParameters): Boolean {
    activeParams = null
    if (instance === this) instance = null
    scheduledWifiOnly = null
    NativeBridge.setDownloadJobRunning(false)
    return activeCount > 0
  }

  override fun onDestroy() {
    if (instance === this) instance = null
    super.onDestroy()
  }

  private fun updateOrFinish(count: Int) {
    val params = activeParams ?: return
    if (count <= 0) {
      activeParams = null
      instance = null
      scheduledWifiOnly = null
      NativeBridge.setDownloadJobRunning(false)
      jobFinished(params, false)
    } else {
      setNotification(
        params,
        NOTIF_ID,
        buildNotification(this, count),
        JOB_END_NOTIFICATION_POLICY_REMOVE,
      )
    }
  }

  companion object {
    private const val JOB_ID = 4401
    private const val NOTIF_ID = 4402
    private const val CHANNEL_ID = "soulseek_downloads"
    private val mainHandler = Handler(Looper.getMainLooper())

    @Volatile private var instance: SoulseekJobService? = null
    @Volatile private var activeCount: Int = 0
    @Volatile private var scheduledWifiOnly: Boolean? = null

    fun onQueueChanged(context: Context, count: Int, wifiOnly: Boolean) {
      val appCtx = context.applicationContext
      mainHandler.post {
        activeCount = count
        val scheduler = appCtx.getSystemService(JobScheduler::class.java) ?: return@post
        if (count <= 0) {
          scheduledWifiOnly = null
          instance?.updateOrFinish(0)
          runCatching { scheduler.cancel(JOB_ID) }
          NativeBridge.setDownloadJobRunning(false)
          return@post
        }
        if (scheduledWifiOnly == wifiOnly) {
          instance?.updateOrFinish(count)
          return@post
        }
        val netReq = NetworkRequest.Builder()
          .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
          .apply {
            if (wifiOnly) addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
          }
          .build()
        val job = JobInfo.Builder(JOB_ID, ComponentName(appCtx, SoulseekJobService::class.java))
          .setUserInitiated(true)
          .setRequiredNetwork(netReq)
          .build()
        if (runCatching { scheduler.schedule(job) }.isSuccess) {
          scheduledWifiOnly = wifiOnly
        }
      }
    }

    private fun ensureChannel(context: Context) {
      val nm = context.getSystemService(NotificationManager::class.java) ?: return
      if (nm.getNotificationChannel(CHANNEL_ID) != null) return
      val ch = NotificationChannel(
        CHANNEL_ID,
        "Soulseek downloads",
        NotificationManager.IMPORTANCE_LOW,
      ).apply {
        description = "Shows progress while Soulseek downloads run."
      }
      nm.createNotificationChannel(ch)
    }

    private fun buildNotification(context: Context, count: Int): Notification {
      ensureChannel(context)
      val open = Intent(context, MainActivity::class.java).apply {
        flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
      }
      val pi = PendingIntent.getActivity(
        context,
        0,
        open,
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
      )
      val text = if (count == 1) "Downloading 1 track from Soulseek…" else "Downloading $count tracks from Soulseek…"
      return Notification.Builder(context, CHANNEL_ID)
        .setSmallIcon(R.mipmap.ic_launcher)
        .setContentTitle("Undertone")
        .setContentText(text)
        .setContentIntent(pi)
        .setOngoing(true)
        .build()
    }
  }
}
