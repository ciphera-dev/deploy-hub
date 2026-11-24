import chatgptMark from "@/assets/images/connectors/chatgpt-mark.png";
import claudeMark from "@/assets/images/connectors/claude-mark.png";
import crowdsecMark from "@/assets/images/connectors/crowdsec-mark.png";
import githubMark from "@/assets/images/connectors/github-mark.png";
import groqMark from "@/assets/images/connectors/groq-mark.png";
import openrouterMark from "@/assets/images/connectors/openrouter-mark.png";
import supabaseMark from "@/assets/images/connectors/supabase-mark.png";
import cloudflareMark from "@/assets/images/cloudflare/cloudflare-mark.png";
import type { IconName } from "@/design-system";

// Connectors.jsx#LIB : catalogue des services. En v0.1 seule la GitHub App est
// branchee ; les autres connecteurs portent la version qui les apporte.

export interface Connector {
  id: string;
  n: string;
  cat: string;
  org?: string;
  img?: string;
  ic?: IconName;
  col: string;
  d: string;
  /** methode d'autorisation affichee (absent = OAuth) */
  key?: string;
  scopes: string[];
  /** version qui apporte le connecteur ; absent = disponible */
  soon?: "v0.2" | "v0.3";
}

export const CATS = ["Tous", "Code source", "DNS et réseau", "Sécurité", "Bases de données", "Notifications", "IA"];

export const LIB: Connector[] = [
  { id: "github", n: "GitHub", cat: "Code source", img: githubMark, col: "#ededed", d: "Déploiement à chaque push et previews par pull request.", scopes: ["Dépôts", "Webhooks", "Checks"] },
  { id: "cloudflare", n: "Cloudflare", cat: "DNS et réseau", img: cloudflareMark, col: "#f38020", d: "DNS automatique, proxy, WAF et certificats d'origine.", scopes: ["DNS", "SSL", "Zones"], soon: "v0.2" },
  { id: "crowdsec", n: "CrowdSec", cat: "Sécurité", key: "crowdsec", img: crowdsecMark, col: "#8b7cf6", d: "Détection d'intrusions et blocklists communautaires sur vos serveurs.", scopes: ["Décisions", "Alertes", "Blocklists"], soon: "v0.3" },
  { id: "supabase", n: "Supabase", cat: "Bases de données", img: supabaseMark, col: "#3ecf8e", d: "Postgres managé, auth et stockage. Variables injectées dans vos projets.", key: "supabase", scopes: ["Projets", "Base", "Auth"], soon: "v0.2" },
  { id: "smtp", n: "E-mail SMTP", cat: "Notifications", ic: "Mail", col: "#7dd3fc", d: "Alertes, rapports et invitations via votre serveur SMTP.", key: "smtp", scopes: ["Envoi"], soon: "v0.2" },
  { id: "openai", n: "ChatGPT", org: "OpenAI", cat: "IA", img: chatgptMark, col: "#10a37f", d: "GPT pour l'analyse des logs et les résumés d'incidents.", key: "sk-...", scopes: ["Chat", "Embeddings"], soon: "v0.3" },
  { id: "claude", n: "Claude", org: "Anthropic", cat: "IA", img: claudeMark, col: "#d97757", d: "Modèles Anthropic pour l'assistant et la revue de configuration.", key: "sk-ant-...", scopes: ["Messages"], soon: "v0.3" },
  { id: "groq", n: "Groq", org: "Groq", cat: "IA", img: groqMark, col: "#f55036", d: "Inférence très rapide pour l'autocomplétion et le tri des alertes.", key: "gsk_...", scopes: ["Chat"], soon: "v0.3" },
  { id: "openrouter", n: "OpenRouter", org: "OpenRouter", cat: "IA", img: openrouterMark, col: "#818cf8", d: "Un seul jeton pour des centaines de modèles, avec repli automatique.", key: "sk-or-...", scopes: ["Chat", "Routage"], soon: "v0.3" },
];

export const rgb = (h: string) => {
  const n = parseInt(h.slice(1), 16);
  return `${(n >> 16) & 255},${(n >> 8) & 255},${n & 255}`;
};
