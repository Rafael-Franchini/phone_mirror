package com.phonemirror.companion

import android.app.Dialog
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import android.view.Gravity
import android.view.View
import android.view.Window
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import java.net.InetAddress

class MainActivity : AppCompatActivity() {

    private lateinit var statusBadge: TextView
    private lateinit var btnStart: Button
    private lateinit var tabConnectBtn: Button
    private lateinit var tabAboutBtn: Button
    private lateinit var connectionView: LinearLayout
    private lateinit var aboutView: LinearLayout
    private var requestDialog: Dialog? = null

    companion object {
        const val GITHUB_URL = "https://github.com/Rafael-Franchini"
        const val LINKEDIN_URL = "https://www.linkedin.com/in/rafael-franchini-37b0a21a4/"
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val rootScroll = ScrollView(this).apply {
            setBackgroundColor(ContextCompat.getColor(this@MainActivity, R.color.ios_bg))
            isFillViewport = true
        }

        val container = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(48, 80, 48, 64)
        }
        rootScroll.addView(container)

        // Header estilo iOS (Large Title)
        val title = TextView(this).apply {
            text = "PhoneMirror"
            textSize = 34f
            typeface = Typeface.create("sans-serif-medium", Typeface.BOLD)
            setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_text_primary))
            setPadding(8, 0, 0, 6)
        }
        container.addView(title)

        val subtitle = TextView(this).apply {
            text = "Espelhamento de Tela & Controle para Jogos"
            textSize = 15f
            setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_text_secondary))
            setPadding(8, 0, 0, 28)
        }
        container.addView(subtitle)

        // Seletor de Abas estilo Segmented Control (Conexão | Sobre)
        val segmentedBar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(12).toFloat()
                setColor(Color.parseColor("#242426"))
            }
            setPadding(dp(4), dp(4), dp(4), dp(4))
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
            ).apply {
                setMargins(0, 0, 0, dp(20))
            }
        }

        tabConnectBtn = Button(this).apply {
            text = "📱 Conexão"
            textSize = 14f
            typeface = Typeface.DEFAULT_BOLD
            isAllCaps = false
            layoutParams = LinearLayout.LayoutParams(0, dp(38), 1f)
            setOnClickListener { switchTab(isAbout = false) }
        }
        tabAboutBtn = Button(this).apply {
            text = "ℹ️ Sobre"
            textSize = 14f
            typeface = Typeface.DEFAULT_BOLD
            isAllCaps = false
            layoutParams = LinearLayout.LayoutParams(0, dp(38), 1f)
            setOnClickListener { switchTab(isAbout = true) }
        }
        segmentedBar.addView(tabConnectBtn)
        segmentedBar.addView(tabAboutBtn)
        container.addView(segmentedBar)

        // ==================== ABA 1: CONEXÃO ====================
        connectionView = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }

        val deviceName = DiscoveryEngine.getDeviceName(this)
        val hasWifi = DiscoveryEngine.getWifiIpAddress() != null
        DiscoveryEngine.start(this)

        val statusCard = createIosCard().apply {
            addView(createSectionHeader("IDENTIFICAÇÃO DO APARELHO"))

            val statusRow = LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(0, 8, 0, 16)
            }

            statusBadge = TextView(this@MainActivity).apply {
                text = if (hasWifi) "● Visível para o PC no Wi-Fi" else "● Conecte-se ao Wi-Fi"
                textSize = 17f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(
                    ContextCompat.getColor(
                        this@MainActivity,
                        if (hasWifi) R.color.ios_green else R.color.ios_blue
                    )
                )
            }
            statusRow.addView(statusBadge)
            addView(statusRow)

            addView(createDivider())
            addView(createKeyValueRow("Nome do Aparelho", deviceName))
            addView(createDivider())
            addView(createKeyValueRow("Conexão Wi-Fi", if (hasWifi) "Descoberta Automática" else "Aguardando Wi-Fi"))
            addView(createDivider())
            addView(createKeyValueRow("Protocolo", "Direct3D 11 / scrcpy / UHID"))
        }
        connectionView.addView(statusCard)

        connectionView.addView(View(this).apply { layoutParams = LinearLayout.LayoutParams(1, 28) })

        val actionCard = createIosCard().apply {
            addView(createSectionHeader("CONTROLE DE TRANSMISSÃO"))

            btnStart = Button(this@MainActivity).apply {
                text = "✓ Aceitar / Liberar Transmissão"
                textSize = 16f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(Color.WHITE)
                isAllCaps = false
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    dp(54)
                ).apply {
                    setMargins(0, 8, 0, 14)
                }
                setOnClickListener {
                    if (DiscoveryEngine.isStreaming) {
                        DiscoveryEngine.stopStreamingFromPhone(this@MainActivity)
                        updateStreamingUi(false, null)
                    } else {
                        DiscoveryEngine.enableManualStreaming(this@MainActivity)
                        updateStreamingUi(true, DiscoveryEngine.connectedPcName)
                    }
                }
            }
            addView(btnStart)

            val btnDev = Button(this@MainActivity).apply {
                text = "⚙️ Ativar Depuração USB / Wi-Fi"
                textSize = 15f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_blue))
                background = ContextCompat.getDrawable(this@MainActivity, R.drawable.bg_ios_button_secondary)
                isAllCaps = false
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    dp(48)
                ).apply {
                    setMargins(0, 0, 0, 10)
                }
                setOnClickListener {
                    try {
                        startActivity(Intent(Settings.ACTION_APPLICATION_DEVELOPMENT_SETTINGS))
                    } catch (_: Exception) {
                        startActivity(Intent(Settings.ACTION_SETTINGS))
                    }
                }
            }
            addView(btnDev)

            val btnAccess = Button(this@MainActivity).apply {
                text = "🎮 Permissão de Toque / Acessibilidade"
                textSize = 15f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_blue))
                background = ContextCompat.getDrawable(this@MainActivity, R.drawable.bg_ios_button_secondary)
                isAllCaps = false
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    dp(48)
                ).apply {
                    setMargins(0, 0, 0, 8)
                }
                setOnClickListener {
                    startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
                }
            }
            addView(btnAccess)
        }
        connectionView.addView(actionCard)

        val footer = TextView(this).apply {
            text = "Como Conectar:\n1. No PC, clique em Conectar no card \"$deviceName\".\n2. Toque no botão verde para aceitar a transmissão ou no botão vermelho para encerrá-la a qualquer momento."
            textSize = 13f
            setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_text_secondary))
            setPadding(12, 20, 12, 10)
        }
        connectionView.addView(footer)
        container.addView(connectionView)

        // ==================== ABA 2: SOBRE ====================
        aboutView = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
        }

        val devCard = createIosCard().apply {
            addView(createSectionHeader("DESENVOLVEDOR & CRÉDITOS"))

            val devTitle = TextView(this@MainActivity).apply {
                text = "Rafael Franchini (R4LF)"
                textSize = 20f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_text_primary))
                setPadding(0, 4, 0, 6)
            }
            addView(devTitle)

            val devRole = TextView(this@MainActivity).apply {
                text = "Engenharia da Computação • Desenvolvedor de Software Mobile & Full-Stack"
                textSize = 14f
                setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_text_secondary))
                setPadding(0, 0, 0, 18)
            }
            addView(devRole)

            addView(createDivider())
            addView(createKeyValueRow("Aplicativo", "PhoneMirror v1.0"))
            addView(createDivider())
            addView(createKeyValueRow("Motor de Vídeo", "Baixa Latência 60/120 FPS"))
            addView(createDivider())
            addView(createKeyValueRow("Descoberta de Rede", "UDP Auto-Scan Wi-Fi"))
        }
        aboutView.addView(devCard)

        aboutView.addView(View(this).apply { layoutParams = LinearLayout.LayoutParams(1, 28) })

        val linksCard = createIosCard().apply {
            addView(createSectionHeader("REDES SOCIAIS & PORTFÓLIO"))

            val btnGithub = Button(this@MainActivity).apply {
                text = "🐙 GitHub • @Rafael-Franchini"
                textSize = 16f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(Color.WHITE)
                background = GradientDrawable().apply {
                    shape = GradientDrawable.RECTANGLE
                    cornerRadius = dp(14).toFloat()
                    setColor(Color.parseColor("#24292E"))
                    setStroke(dp(1), Color.parseColor("#444D56"))
                }
                isAllCaps = false
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    dp(52)
                ).apply {
                    setMargins(0, 8, 0, 12)
                }
                setOnClickListener {
                    openUrl(GITHUB_URL)
                }
            }
            addView(btnGithub)

            val btnLinkedin = Button(this@MainActivity).apply {
                text = "💼 LinkedIn • Rafael Franchini"
                textSize = 16f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(Color.WHITE)
                background = GradientDrawable().apply {
                    shape = GradientDrawable.RECTANGLE
                    cornerRadius = dp(14).toFloat()
                    setColor(Color.parseColor("#0A66C2"))
                }
                isAllCaps = false
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    dp(52)
                ).apply {
                    setMargins(0, 0, 0, 12)
                }
                setOnClickListener {
                    openUrl(LINKEDIN_URL)
                }
            }
            addView(btnLinkedin)

            val btnPrivacy = Button(this@MainActivity).apply {
                text = "🔒 Política de Privacidade (LGPD)"
                textSize = 15f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_blue))
                background = ContextCompat.getDrawable(this@MainActivity, R.drawable.bg_ios_button_secondary)
                isAllCaps = false
                layoutParams = LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    dp(48)
                ).apply {
                    setMargins(0, 0, 0, 8)
                }
                setOnClickListener {
                    showPrivacyPolicyDialog()
                }
            }
            addView(btnPrivacy)
        }
        aboutView.addView(linksCard)

        val aboutFooter = TextView(this).apply {
            text = "© 2026 Rafael Franchini. Todos os direitos reservados.\ngithub.com/Rafael-Franchini"
            textSize = 13f
            gravity = Gravity.CENTER
            setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_text_secondary))
            setPadding(12, 24, 12, 10)
        }
        aboutView.addView(aboutFooter)
        container.addView(aboutView)

        setContentView(rootScroll)
        switchTab(isAbout = false)
        updateStreamingUi(DiscoveryEngine.isStreaming, DiscoveryEngine.connectedPcName)
        checkIntentForRequest(intent)
    }

    private fun switchTab(isAbout: Boolean) {
        connectionView.visibility = if (isAbout) View.GONE else View.VISIBLE
        aboutView.visibility = if (isAbout) View.VISIBLE else View.GONE

        val activeBg = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = dp(9).toFloat()
            setColor(ContextCompat.getColor(this@MainActivity, R.color.ios_blue))
        }
        val inactiveBg = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = dp(9).toFloat()
            setColor(Color.TRANSPARENT)
        }

        tabConnectBtn.background = if (!isAbout) activeBg else inactiveBg
        tabConnectBtn.setTextColor(if (!isAbout) Color.WHITE else Color.parseColor("#98989D"))

        tabAboutBtn.background = if (isAbout) activeBg else inactiveBg
        tabAboutBtn.setTextColor(if (isAbout) Color.WHITE else Color.parseColor("#98989D"))
    }

    private fun openUrl(url: String) {
        try {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
        } catch (_: Exception) {}
    }

    private fun showPrivacyPolicyDialog() {
        val dialog = Dialog(this)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)
        dialog.window?.setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))

        val card = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(22).toFloat()
                setColor(Color.parseColor("#2C2C2E"))
                setStroke(dp(1), Color.parseColor("#3A3A3C"))
            }
            setPadding(dp(22), dp(24), dp(22), dp(20))
        }

        val header = TextView(this).apply {
            text = "🔒 Política de Privacidade & LGPD"
            textSize = 19f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Color.WHITE)
            setPadding(0, 0, 0, dp(12))
        }
        card.addView(header)

        val scroll = ScrollView(this).apply {
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                (resources.displayMetrics.heightPixels * 0.55).toInt()
            )
        }

        val policyText = TextView(this).apply {
            text = """
                1. PROCESSAMENTO 100% LOCAL (ZERO NUVEM)
                O PhoneMirror transmite vídeo e comandos exclusivamente dentro da sua rede Wi-Fi local (LAN) ou cabo USB direto entre o seu celular e o seu computador. Nenhum vídeo, imagem ou dado pessoal é enviado para servidores externos.

                2. DADOS UTILIZADOS NA REDE LOCAL
                • Nome público do aparelho e IP local privado (ex.: 192.168.x.x): usados unicamente para descobrir o celular automaticamente no PC sem precisar digitar o IP manualmente.
                • Transmissão de tela (MediaProjection / ADB): iniciada somente após o seu aceite explícito na tela do celular, podendo ser interrompida a qualquer momento no botão "Desativar Transmissão".

                3. SERVIÇO DE ACESSIBILIDADE (OPCIONAL)
                • O serviço InputAccessibilityService é usado estritamente para executar os toques e comandos enviados pelo próprio usuário a partir do PC.
                • O aplicativo NÃO lê senhas, NÃO captura textos ou notificações e NÃO monitora outros aplicativos.

                4. DIREITOS DO TITULAR (LGPD - LEI Nº 13.709/2018)
                Como o PhoneMirror não coleta nem armazena dados em nuvem, você mantém controle total no próprio aparelho e pode revogar permissões ou encerrar a transmissão instantaneamente.

                5. CONTROLADOR E CONTATO
                • Desenvolvedor: Rafael Franchini (R4LF)
                • GitHub: github.com/Rafael-Franchini
                • LinkedIn: linkedin.com/in/rafael-franchini-37b0a21a4
            """.trimIndent()
            textSize = 14f
            setLineSpacing(0f, 1.25f)
            setTextColor(Color.parseColor("#D1D1D6"))
        }
        scroll.addView(policyText)
        card.addView(scroll)

        val btnClose = Button(this).apply {
            text = "Fechar"
            textSize = 16f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Color.WHITE)
            isAllCaps = false
            background = ContextCompat.getDrawable(this@MainActivity, R.drawable.bg_ios_button)
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                dp(48)
            ).apply {
                setMargins(0, dp(16), 0, 0)
            }
            setOnClickListener { dialog.dismiss() }
        }
        card.addView(btnClose)

        dialog.setContentView(card)
        val width = (resources.displayMetrics.widthPixels * 0.90).toInt()
        dialog.window?.setLayout(width, LinearLayout.LayoutParams.WRAP_CONTENT)
        dialog.show()
    }

    override fun onNewIntent(intent: Intent?) {
        super.onNewIntent(intent)
        setIntent(intent)
        checkIntentForRequest(intent)
    }

    override fun onResume() {
        super.onResume()
        DiscoveryEngine.start(this)
        DiscoveryEngine.onConnectionRequest = { pcName, pcAddr, pcPort ->
            showConnectionRequestDialog(pcName, pcAddr, pcPort)
        }
        DiscoveryEngine.onStreamingStateChanged = { streaming, pcName ->
            updateStreamingUi(streaming, pcName)
        }
        updateStreamingUi(DiscoveryEngine.isStreaming, DiscoveryEngine.connectedPcName)
        DiscoveryEngine.announceNow(this, fullSubnetSweep = true)
    }

    override fun onPause() {
        super.onPause()
        DiscoveryEngine.onConnectionRequest = null
        DiscoveryEngine.onStreamingStateChanged = null
    }

    private fun checkIntentForRequest(intent: Intent?) {
        val pcName = intent?.getStringExtra("request_pc_name") ?: return
        val pcIp = intent.getStringExtra("request_pc_ip") ?: return
        val pcPort = intent.getIntExtra("request_pc_port", 45455)
        intent.removeExtra("request_pc_name")
        Thread {
            try {
                val addr = InetAddress.getByName(pcIp)
                runOnUiThread {
                    showConnectionRequestDialog(pcName, addr, pcPort)
                }
            } catch (_: Exception) {}
        }.start()
    }

    private fun showConnectionRequestDialog(pcName: String, pcAddress: InetAddress, pcPort: Int) {
        requestDialog?.dismiss()
        switchTab(isAbout = false)

        val dialog = Dialog(this)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)
        dialog.setCancelable(false)
        dialog.window?.setBackgroundDrawable(ColorDrawable(Color.TRANSPARENT))

        val card = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(22).toFloat()
                setColor(Color.parseColor("#2C2C2E"))
                setStroke(dp(1), Color.parseColor("#3A3A3C"))
            }
            setPadding(dp(24), dp(26), dp(24), dp(22))
        }

        val iconView = TextView(this).apply {
            text = "💻"
            textSize = 38f
            gravity = Gravity.CENTER
            setPadding(0, 0, 0, dp(10))
        }
        card.addView(iconView)

        val titleView = TextView(this).apply {
            text = "Aceitar Transmissão?"
            textSize = 21f
            typeface = Typeface.DEFAULT_BOLD
            gravity = Gravity.CENTER
            setTextColor(Color.WHITE)
            setPadding(0, 0, 0, dp(8))
        }
        card.addView(titleView)

        val msgView = TextView(this).apply {
            text = "O computador \"$pcName\" quer iniciar o espelhamento da tela deste celular via Wi-Fi."
            textSize = 15f
            gravity = Gravity.CENTER
            setTextColor(Color.parseColor("#AEAEB2"))
            setPadding(dp(4), 0, dp(4), dp(22))
        }
        card.addView(msgView)

        val btnAccept = Button(this).apply {
            text = "✓ Aceitar e Transmitir Tela"
            textSize = 16f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Color.WHITE)
            isAllCaps = false
            background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(14).toFloat()
                setColor(Color.parseColor("#30D158"))
            }
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                dp(52)
            ).apply {
                setMargins(0, 0, 0, dp(10))
            }
            setOnClickListener {
                dialog.dismiss()
                DiscoveryEngine.respondToPcRequest(this@MainActivity, true, pcName, pcAddress, pcPort)
            }
        }
        card.addView(btnAccept)

        val btnReject = Button(this).apply {
            text = "✕ Recusar Conexão"
            textSize = 15f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Color.parseColor("#FF453A"))
            isAllCaps = false
            background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(14).toFloat()
                setColor(Color.parseColor("#3A2325"))
            }
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                dp(48)
            )
            setOnClickListener {
                dialog.dismiss()
                DiscoveryEngine.respondToPcRequest(this@MainActivity, false, pcName, pcAddress, pcPort)
            }
        }
        card.addView(btnReject)

        dialog.setContentView(card)
        val width = (resources.displayMetrics.widthPixels * 0.88).toInt()
        dialog.window?.setLayout(width, LinearLayout.LayoutParams.WRAP_CONTENT)
        requestDialog = dialog
        dialog.show()
    }

    private fun updateStreamingUi(streaming: Boolean, pcName: String?) {
        val hasWifi = DiscoveryEngine.getWifiIpAddress() != null
        if (streaming) {
            val target = if (!pcName.isNullOrBlank()) pcName else "o PC"
            statusBadge.text = "● Transmitindo para $target"
            statusBadge.setTextColor(ContextCompat.getColor(this, R.color.ios_green))
            btnStart.text = "⏹ Desativar Transmissão"
            btnStart.setTextColor(Color.WHITE)
            btnStart.background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(14).toFloat()
                setColor(Color.parseColor("#FF453A"))
            }
            btnStart.isEnabled = true
        } else {
            statusBadge.text = if (hasWifi) "● Visível para o PC no Wi-Fi" else "● Conecte-se ao Wi-Fi"
            statusBadge.setTextColor(
                ContextCompat.getColor(
                    this,
                    if (hasWifi) R.color.ios_green else R.color.ios_blue
                )
            )
            btnStart.text = "✓ Aceitar / Liberar Transmissão"
            btnStart.setTextColor(Color.WHITE)
            btnStart.background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(14).toFloat()
                setColor(Color.parseColor("#30D158"))
            }
            btnStart.isEnabled = true
        }
    }

    private fun createIosCard(): LinearLayout {
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            background = ContextCompat.getDrawable(this@MainActivity, R.drawable.bg_ios_card)
            setPadding(40, 28, 40, 28)
        }
    }

    private fun createSectionHeader(title: String): TextView {
        return TextView(this).apply {
            text = title
            textSize = 13f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_text_secondary))
            setPadding(0, 0, 0, 14)
        }
    }

    private fun createKeyValueRow(label: String, value: String): LinearLayout {
        return LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(0, 16, 0, 16)

            val tvLabel = TextView(this@MainActivity).apply {
                text = label
                textSize = 15f
                setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_text_primary))
                layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
            }
            addView(tvLabel)

            val tvValue = TextView(this@MainActivity).apply {
                text = value
                textSize = 15f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(ContextCompat.getColor(this@MainActivity, R.color.ios_text_secondary))
            }
            addView(tvValue)
        }
    }

    private fun createDivider(): View {
        return View(this).apply {
            setBackgroundColor(ContextCompat.getColor(this@MainActivity, R.color.ios_separator))
            layoutParams = LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 1)
        }
    }

    private fun dp(value: Int): Int {
        return (value * resources.displayMetrics.density).toInt()
    }
}
