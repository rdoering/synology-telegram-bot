package metrics

// Bot metrics. Label values come from small fixed sets to keep cardinality low.
var (
	BuildInfo = NewGauge("stb_build_info", "Build information (value is always 1)", "version")
	StartTime = NewGauge("stb_start_time_seconds", "Unix time the process started")

	UpdatesReceived = NewCounter("stb_telegram_updates_received_total",
		"Telegram updates received from users", "type", "authorized")
	Commands     = NewCounter("stb_commands_total", "Bot commands received from the authorized chat", "cmd")
	Unauthorized = NewCounter("stb_unauthorized_attempts_total", "Requests from chats that are not allowed")

	MessagesSent = NewCounter("stb_telegram_messages_sent_total",
		"Calls made to the Telegram API (send, edit, delete, answers)", "kind")
	SendErrors = NewCounter("stb_telegram_send_errors_total", "Failed Telegram API calls", "kind")

	SynologyRequests = NewCounter("stb_synology_requests_total",
		"Synology DSM operations by outcome", "op", "result")
	SynologyErrors = NewCounter("stb_synology_errors_total",
		"Failed Synology DSM operations by DSM error code (other = non-DSM error)", "code")
	SynologyLastSuccess = NewGauge("stb_synology_last_success_timestamp_seconds",
		"Unix time of the last successful Synology DSM operation")
	SynologyDuration = NewSummary("stb_synology_request_duration_seconds",
		"Duration of Synology DSM operations", "op")

	BaoPolls = NewCounter("stb_bao_polls_total", "OpenBao seal status polls by outcome", "result")
	BaoState = NewGauge("stb_bao_state", "Current OpenBao state (one-hot)", "state")
	Unseals  = NewCounter("stb_unseal_attempts_total", "Unseal attempts by outcome", "result")
)

// Known label values; Init makes the series visible before the first event.
var (
	CommandNames = []string{"start", "ssh", "ssh_on", "ssh_off", "seal_status", "unseal", "other"}
	BaoStates    = []string{"unreachable", "not_initialized", "sealed", "unsealed"}
	UnsealResult = []string{"ok", "partial", "invalid", "decrypt_failed", "rejected", "expired"}
	SendKinds    = []string{"send", "edit", "delete", "answer_callback", "answer_inline"}
	SSHOps       = []string{"ssh_status", "ssh_set"}
)

func init() {
	for _, c := range CommandNames {
		Commands.Init(c)
	}
	for _, k := range SendKinds {
		MessagesSent.Init(k)
		SendErrors.Init(k)
	}
	for _, o := range SSHOps {
		for _, r := range []string{"ok", "error"} {
			SynologyRequests.Init(o, r)
		}
	}
	for _, r := range UnsealResult {
		Unseals.Init(r)
	}
	for _, r := range []string{"ok", "error"} {
		BaoPolls.Init(r)
	}
	Unauthorized.Init()
	for _, t := range []string{"message", "callback", "inline"} {
		for _, a := range []string{"true", "false", "na"} {
			UpdatesReceived.Init(t, a)
		}
	}
}
