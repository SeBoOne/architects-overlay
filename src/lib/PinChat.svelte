<script lang="ts">
// M6-F1 (t_58d46923): DealChat-Pin-Renderer — registrierter Content-Typ
// 'deal-chat'. Zeigt den Chat-Verlauf im Pin-Körper und (nur bei
// interaktivem Spotlight, M4-E/M5-Vertrag) einen Inline-Composer, der
// direkt über send_negotiation_message schreibt — ohne den Deal im
// Overlay öffnen zu müssen. Der Composer lebt im SPOTLIGHT-Webview (die
// Pin-Box ist DOM dort); das Design-Kontrakt „Anzeige + unsichtbare
// Interaktionsebene“ ist strukturell erfüllt, sobald die Box nicht-
// interaktiv ist (Pins-only/Click-Through) rendert nur der Verlauf.
import { invoke } from '@tauri-apps/api/core';
import { t, trError } from './i18n';
import type { PinEntry } from './pins';

type DealChatPayload = {
	hash: string;
	id_negotiation: number;
	who: string;
	own_username: string;
	ended: boolean;
};

type NegotiationMessage = {
	id: number;
	message: string | null;
	user_username: string;
	user_name: string;
	date_added: number;
};

let { pin, interactive = false }: { pin: PinEntry; interactive?: boolean } = $props();

const payload = $derived((pin.content ?? {}) as Partial<DealChatPayload>);
const hash = $derived(payload.hash ?? '');
const idNeg = $derived(Number(payload.id_negotiation ?? 0));
const who = $derived(payload.who ?? '—');
const ownUsername = $derived(payload.own_username ?? '');
const ended = $derived(!!payload.ended);

let msgs = $state<NegotiationMessage[]>([]);
let loading = $state(false);
let err = $state<string | null>(null);
let draft = $state('');
let busy = $state(false);
let scroller = $state<HTMLElement | null>(null);
let poller: ReturnType<typeof setInterval> | null = null;

async function load(bg = false) {
	if (!hash && !idNeg) return;
	loading = !bg;
	try {
		const r = await invoke<{ messages: NegotiationMessage[]; note?: string | null }>(
			'get_negotiation_messages',
			{ hash, id_negotiation: idNeg }
		);
		msgs = r.messages ?? [];
		if (r.note) err = r.note;
	} catch (e) {
		err = trError(e);
	}
	loading = false;
	queueMicrotask(() => {
		if (scroller) scroller.scrollTop = scroller.scrollHeight;
	});
}

async function send() {
	if (busy || ended) return;
	const text = draft.trim();
	if (!text) return;
	busy = true;
	err = null;
	try {
		await invoke('send_negotiation_message', { id_negotiation: idNeg, hash, message: text });
		draft = '';
		await load(true);
	} catch (e) {
		// Kein Retry — Meldung (429/403-Pfad wie der Chat im Trades-Reiter).
		err = trError(e);
	}
	busy = false;
}

function onKey(e: KeyboardEvent) {
	// Escape NICHT fressen — der schließt die Pin-Popover-Kette im Overlay.
	if (e.key === 'Enter' && !e.shiftKey) {
		e.preventDefault();
		void send();
	}
}

function msgTime(ts: number): string {
	if (!ts) return '';
	const d = new Date(ts * 1000);
	return d.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' });
}

$effect(() => {
	void load();
	// Live-Nachziehen nur im interaktiven Overlay (Pins-only ist reine
	// Anzeige — Poll-Last sparen, M5-Gate-Mechanik).
	if (interactive) {
		poller = setInterval(() => void load(true), 15000);
		return () => {
			if (poller) clearInterval(poller);
		};
	}
});
</script>

<div class="pin-chat" role="log" aria-label={$t('pins.chat_title')}>
	<div class="chat-list" bind:this={scroller}>
		{#if loading}
			<div class="chat-note">…</div>
		{:else if msgs.length === 0 && !err}
			<div class="chat-note">—</div>
		{:else}
			{#each msgs as m (m.id)}
				<div class="msg" class:own={m.user_username === ownUsername}>
					<span class="who">{m.user_username || m.user_name || '—'}</span>
					<span class="txt">{m.message ?? ''}</span>
					<span class="when">{msgTime(m.date_added)}</span>
				</div>
			{/each}
		{/if}
		{#if err}<div class="chat-note err">{err}</div>{/if}
	</div>
	{#if interactive && !ended}
		<form
			class="chat-form"
			onsubmit={(e) => {
				e.preventDefault();
				void send();
			}}
		>
			<input
				type="text"
				bind:value={draft}
				onkeydown={onKey}
				placeholder={$t('trades.chat_placeholder', { who })}
				disabled={busy}
			/>
			<button type="submit" class="chat-send" disabled={busy || !draft.trim()} title={$t('trades.send_chat_title')}>
				{$t('trades.send')}
			</button>
		</form>
	{:else if ended}
		<div class="chat-note">{$t('trades.readonly_note')}</div>
	{/if}
</div>

<style>
	.pin-chat {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.chat-list {
		display: flex;
		flex-direction: column;
		gap: 5px;
		max-height: 210px;
		min-height: 60px;
		overflow-y: auto;
		padding-right: 2px;
	}
	.msg {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		background: var(--panel2);
		border: 1px solid var(--lineb);
		padding: 4px 7px;
		max-width: 88%;
	}
	.msg.own {
		align-self: flex-end;
		border-color: var(--accent);
	}
	.msg .who {
		font: 700 9px/1.2 var(--font-mono, monospace);
		letter-spacing: 0.08em;
		text-transform: uppercase;
		color: var(--fgdim);
	}
	.msg.own .who {
		color: var(--accent);
	}
	.msg .txt {
		font-size: 12px;
		line-height: 1.35;
		color: var(--fg);
		white-space: pre-wrap;
		word-break: break-word;
	}
	.msg .when {
		align-self: flex-end;
		font: 500 9px/1 var(--font-mono, monospace);
		color: var(--fgdim);
		margin-top: 2px;
	}
	.chat-note {
		font: 500 10.5px/1.4 var(--font-mono, monospace);
		color: var(--fgdim);
	}
	.chat-note.err {
		color: var(--sell);
		white-space: pre-wrap;
	}
	.chat-form {
		display: flex;
		gap: 4px;
	}
	.chat-form input {
		flex: 1;
		min-width: 0;
		background: var(--panel3);
		border: 1px solid var(--lineb);
		color: var(--fg);
		font: 500 12px/1.2 var(--font-mono, monospace);
		padding: 5px 7px;
	}
	.chat-form input:focus {
		outline: none;
		border-color: var(--accent);
	}
	.chat-send {
		background: var(--panel2);
		border: 1px solid var(--lineb);
		color: var(--fg);
		font: 700 10px/1 var(--font-mono, monospace);
		letter-spacing: 0.08em;
		text-transform: uppercase;
		padding: 0 9px;
		cursor: pointer;
	}
	.chat-send:hover:not(:disabled) {
		border-color: var(--accent);
		color: var(--accent);
	}
	.chat-send:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
