package com.phonemirror.companion

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.provider.Settings
import androidx.core.app.NotificationCompat
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.NetworkInterface
import java.util.Collections
import java.util.concurrent.atomic.AtomicBoolean

object DiscoveryEngine {
    const val DISCOVERY_PORT = 45454
    const val PC_NOTIFY_PORT = 45455
    private val running = AtomicBoolean(false)
    private var socket: DatagramSocket? = null
    private val mainHandler = Handler(Looper.getMainLooper())

    @Volatile
    var isStreaming: Boolean = false
        private set

    @Volatile
    var connectedPcName: String? = null
        private set

    @Volatile
    var lastPcAddress: InetAddress? = null
        private set

    @Volatile
    private var pendingDecision: String? = null // "ACCEPT", "REJECT", or null

    @Volatile
    private var dialogOpen: Boolean = false

    var onConnectionRequest: ((pcName: String, pcAddress: InetAddress, pcPort: Int) -> Unit)? = null
    var onStreamingStateChanged: ((streaming: Boolean, pcName: String?) -> Unit)? = null

    fun getDeviceName(context: Context): String {
        try {
            val globalName = Settings.Global.getString(context.contentResolver, "device_name")
            if (!globalName.isNullOrBlank()) return globalName.replace("|", "-")
        } catch (_: Exception) {}
        val manufacturer = Build.MANUFACTURER?.replaceFirstChar { it.uppercase() } ?: ""
        val model = Build.MODEL ?: "Android"
        val combined = if (model.startsWith(manufacturer, ignoreCase = true)) {
            model
        } else {
            "$manufacturer $model".trim()
        }
        return combined.replace("|", "-")
    }

    fun getWifiIpAddress(): String? {
        try {
            val interfaces = Collections.list(NetworkInterface.getNetworkInterfaces())
            for (intf in interfaces) {
                if (!intf.isUp || intf.isLoopback) continue
                if (intf.name.contains("wlan") || intf.name.contains("ap") || intf.name.contains("eth")) {
                    val addrs = Collections.list(intf.inetAddresses)
                    for (addr in addrs) {
                        val host = addr.hostAddress ?: continue
                        if (!addr.isLoopbackAddress && !host.contains(':')) {
                            return host
                        }
                    }
                }
            }
        } catch (_: Exception) {}
        return null
    }

    fun buildPayload(context: Context): String? {
        val ip = getWifiIpAddress() ?: return null
        val name = getDeviceName(context)
        return "PHONEMIRROR_HERE|$name|$ip|5555"
    }

    fun enableManualStreaming(context: Context) {
        isStreaming = true
        pendingDecision = "ACCEPT"
        dialogOpen = false
        mainHandler.post {
            onStreamingStateChanged?.invoke(true, connectedPcName)
        }
        announceNow(context, fullSubnetSweep = false)
    }

    fun respondToPcRequest(context: Context, accepted: Boolean, pcName: String, pcAddress: InetAddress, pcPort: Int) {
        dialogOpen = false
        pendingDecision = if (accepted) "ACCEPT" else "REJECT"
        lastPcAddress = pcAddress
        if (accepted) {
            isStreaming = true
            connectedPcName = pcName
        } else {
            isStreaming = false
            connectedPcName = null
        }

        mainHandler.post {
            onStreamingStateChanged?.invoke(isStreaming, connectedPcName)
        }

        Thread {
            try {
                val reply = if (accepted) "PHONEMIRROR_ACCEPT" else "PHONEMIRROR_REJECT"
                val data = reply.toByteArray(Charsets.UTF_8)
                val activeSrv = socket
                if (activeSrv != null && !activeSrv.isClosed) {
                    activeSrv.send(DatagramPacket(data, data.size, pcAddress, pcPort))
                } else {
                    DatagramSocket().use { s ->
                        s.send(DatagramPacket(data, data.size, pcAddress, pcPort))
                    }
                }
            } catch (_: Exception) {}
        }.apply { isDaemon = true; start() }
    }

    fun stopStreamingFromPhone(context: Context) {
        val appCtx = context.applicationContext
        isStreaming = false
        connectedPcName = null
        pendingDecision = null
        dialogOpen = false
        try {
            appCtx.stopService(Intent(appCtx, ScreenCaptureService::class.java))
        } catch (_: Exception) {}

        mainHandler.post {
            onStreamingStateChanged?.invoke(false, null)
        }

        Thread {
            try {
                val ip = getWifiIpAddress() ?: "0.0.0.0"
                val name = getDeviceName(appCtx)
                val msg = "PHONEMIRROR_STOP|$name|$ip"
                val data = msg.toByteArray(Charsets.UTF_8)
                val activeSrv = socket
                if (activeSrv != null && !activeSrv.isClosed) {
                    lastPcAddress?.let { pcAddr ->
                        try {
                            activeSrv.send(DatagramPacket(data, data.size, pcAddr, PC_NOTIFY_PORT))
                        } catch (_: Exception) {}
                    }
                }
                DatagramSocket().use { s ->
                    s.broadcast = true
                    lastPcAddress?.let { pcAddr ->
                        try {
                            s.send(DatagramPacket(data, data.size, pcAddr, PC_NOTIFY_PORT))
                        } catch (_: Exception) {}
                    }
                    try {
                        s.send(DatagramPacket(data, data.size, InetAddress.getByName("255.255.255.255"), PC_NOTIFY_PORT))
                    } catch (_: Exception) {}
                }
            } catch (_: Exception) {}
        }.apply { isDaemon = true; start() }
    }

    fun start(context: Context) {
        val appCtx = context.applicationContext
        if (running.getAndSet(true)) return

        // Thread 1: Escuta comandos do PC na porta 45454 (Descoberta, Pedido de Transmissão, Parada)
        Thread {
            while (running.get()) {
                var srv: DatagramSocket? = null
                try {
                    srv = DatagramSocket(null).apply {
                        reuseAddress = true
                        broadcast = true
                        bind(InetSocketAddress(DISCOVERY_PORT))
                    }
                    socket = srv
                    val buf = ByteArray(512)
                    while (running.get()) {
                        val pkt = DatagramPacket(buf, buf.size)
                        srv.receive(pkt)
                        val msg = String(pkt.data, 0, pkt.length, Charsets.UTF_8).trim()
                        when {
                            msg.startsWith("PHONEMIRROR_DISCOVER") -> {
                                val payload = buildPayload(appCtx) ?: continue
                                val data = payload.toByteArray(Charsets.UTF_8)
                                srv.send(DatagramPacket(data, data.size, pkt.address, pkt.port))
                                try {
                                    srv.send(DatagramPacket(data, data.size, pkt.address, PC_NOTIFY_PORT))
                                } catch (_: Exception) {}
                            }
                            msg.startsWith("PHONEMIRROR_REQUEST_STREAM") -> {
                                val parts = msg.split("|")
                                val pcName = if (parts.size > 1 && parts[1].isNotBlank()) parts[1].trim() else "Computador PC"
                                lastPcAddress = pkt.address

                                if (isStreaming || pendingDecision == "ACCEPT") {
                                    isStreaming = true
                                    connectedPcName = pcName
                                    val data = "PHONEMIRROR_ACCEPT".toByteArray(Charsets.UTF_8)
                                    srv.send(DatagramPacket(data, data.size, pkt.address, pkt.port))
                                    try {
                                        srv.send(DatagramPacket(data, data.size, pkt.address, PC_NOTIFY_PORT))
                                    } catch (_: Exception) {}
                                    mainHandler.post {
                                        onStreamingStateChanged?.invoke(true, pcName)
                                    }
                                } else if (pendingDecision == "REJECT") {
                                    val data = "PHONEMIRROR_REJECT".toByteArray(Charsets.UTF_8)
                                    srv.send(DatagramPacket(data, data.size, pkt.address, pkt.port))
                                    pendingDecision = null
                                } else if (!dialogOpen) {
                                    dialogOpen = true
                                    mainHandler.post {
                                        val callback = onConnectionRequest
                                        if (callback != null) {
                                            callback.invoke(pcName, pkt.address, pkt.port)
                                        } else {
                                            try {
                                                val i = Intent(appCtx, MainActivity::class.java).apply {
                                                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                                                    putExtra("request_pc_name", pcName)
                                                    putExtra("request_pc_ip", pkt.address.hostAddress)
                                                    putExtra("request_pc_port", pkt.port)
                                                }
                                                appCtx.startActivity(i)
                                            } catch (_: Exception) {
                                                dialogOpen = false
                                            }
                                        }
                                    }
                                }
                            }
                            msg.startsWith("PHONEMIRROR_PING") -> {
                                val reply = if (isStreaming) "PHONEMIRROR_OK" else "PHONEMIRROR_STOP"
                                val data = reply.toByteArray(Charsets.UTF_8)
                                srv.send(DatagramPacket(data, data.size, pkt.address, pkt.port))
                            }
                            msg.startsWith("PHONEMIRROR_PC_STOP") -> {
                                isStreaming = false
                                connectedPcName = null
                                pendingDecision = null
                                dialogOpen = false
                                try {
                                    appCtx.stopService(Intent(appCtx, ScreenCaptureService::class.java))
                                } catch (_: Exception) {}
                                mainHandler.post {
                                    onStreamingStateChanged?.invoke(false, null)
                                }
                            }
                        }
                    }
                } catch (_: Exception) {
                    Thread.sleep(1000)
                } finally {
                    try { srv?.close() } catch (_: Exception) {}
                }
            }
        }.apply { isDaemon = true; start() }

        // Thread 2: Anúncio periódico via broadcast para o PC
        Thread {
            while (running.get()) {
                announceNow(appCtx, fullSubnetSweep = false)
                try { Thread.sleep(2500) } catch (_: Exception) {}
            }
        }.apply { isDaemon = true; start() }
    }

    fun announceNow(context: Context, fullSubnetSweep: Boolean = true) {
        val appCtx = context.applicationContext
        Thread {
            try {
                val payload = buildPayload(appCtx) ?: return@Thread
                val ip = getWifiIpAddress() ?: return@Thread
                val data = payload.toByteArray(Charsets.UTF_8)
                DatagramSocket().use { s ->
                    s.broadcast = true
                    try {
                        s.send(DatagramPacket(data, data.size, InetAddress.getByName("255.255.255.255"), PC_NOTIFY_PORT))
                    } catch (_: Exception) {}

                    val parts = ip.split(".")
                    if (parts.size == 4) {
                        val prefix = "${parts[0]}.${parts[1]}.${parts[2]}"
                        try {
                            s.send(DatagramPacket(data, data.size, InetAddress.getByName("$prefix.255"), PC_NOTIFY_PORT))
                        } catch (_: Exception) {}

                        if (fullSubnetSweep) {
                            for (host in 1..254) {
                                val targetIp = "$prefix.$host"
                                if (targetIp == ip) continue
                                try {
                                    s.send(DatagramPacket(data, data.size, InetAddress.getByName(targetIp), PC_NOTIFY_PORT))
                                } catch (_: Exception) {}
                            }
                        }
                    }
                }
            } catch (_: Exception) {}
        }.apply { isDaemon = true; start() }
    }
}

class ScreenCaptureService : Service() {

    companion object {
        const val ACTION_STOP_STREAM = "com.phonemirror.companion.STOP_STREAM"
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP_STREAM) {
            DiscoveryEngine.stopStreamingFromPhone(this)
            stopSelf()
            return START_NOT_STICKY
        }
        return START_STICKY
    }

    override fun onCreate() {
        super.onCreate()
        DiscoveryEngine.start(this)
        val channelId = "phone_mirror_stream"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                channelId,
                "PhoneMirror Transmissão",
                NotificationManager.IMPORTANCE_LOW
            )
            val manager = getSystemService(NotificationManager::class.java)
            manager?.createNotificationChannel(channel)
        }

        val stopIntent = Intent(this, ScreenCaptureService::class.java).apply {
            action = ACTION_STOP_STREAM
        }
        val stopPending = PendingIntent.getService(
            this,
            0,
            stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val deviceName = DiscoveryEngine.getDeviceName(this)
        val notification: Notification = NotificationCompat.Builder(this, channelId)
            .setContentTitle("PhoneMirror Ativo ($deviceName)")
            .setContentText("Transmitindo tela para o PC via Wi-Fi...")
            .setSmallIcon(android.R.drawable.ic_menu_camera)
            .addAction(android.R.drawable.ic_menu_close_clear_cancel, "Desativar Transmissão", stopPending)
            .build()

        startForeground(101, notification)
    }
}
