/** Ready-to-paste command texts for chat bots that can call a URL. See docs/chat-bots.md. */
export interface BotSnippet {
  bot: string;
  text: string;
  note: string;
}

export function botUrl(base: string, key: string, trigger: string): string {
  return `${base}/${encodeURIComponent(trigger)}?key=${encodeURIComponent(key)}`;
}

export function botSnippets(base: string, key: string, trigger: string): BotSnippet[] {
  const url = botUrl(base, key, trigger);
  return [
    {
      bot: "Nightbot",
      text: `$(urlfetch ${url}&args=$(querystring))`,
      note: "Nightbot 會自己帶上點歌者和身分（實況主、管理員、VIP、訂閱者）。",
    },
    {
      bot: "StreamElements",
      text: `$(customapi ${url}&user=$(queryescape $(sender))&args=$(queryescape $(1:)))`,
      note: "StreamElements 不會帶身分；要限制誰能用，在 StreamElements 的指令權限設定。",
    },
    {
      bot: "Fossabot",
      text: `$(customapi ${url}&args=$(querystring))`,
      note: "Fossabot 會帶上點歌者，但不帶身分；權限在 Fossabot 的指令設定裡限制。",
    },
    {
      bot: "Streamer.bot",
      text: `${url}&user=%user%&args=%rawInputUrlEncoded%`,
      note: "用「Fetch URL」子動作呼叫這個網址，再用「Send Message」把回應貼到聊天室。",
    },
  ];
}
