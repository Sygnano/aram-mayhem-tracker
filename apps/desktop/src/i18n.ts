import portuguese from "./locales/pt-BR.json";

export type Language = "auto" | "en" | "pt-BR";
export type Locale = "en" | "pt-BR";
export type Message = keyof typeof portuguese;

export function resolveLocale(language: Language, systemLanguages: readonly string[]): Locale {
  if (language !== "auto") return language;
  // Use the system's preferred language. Unsupported languages fall back to existing English.
  return systemLanguages[0]?.replaceAll("_", "-").toLowerCase().startsWith("pt") ? "pt-BR" : "en";
}

let locale: Locale = resolveLocale("auto", navigator.languages);
const subscribers = new Set<() => void>();
export const subscribeLocale = (listener: () => void) => {
  subscribers.add(listener);
  return () => { subscribers.delete(listener); };
};
export const currentLocale = () => locale;
export function setLanguage(language: Language) {
  const next = resolveLocale(language, navigator.languages);
  document.documentElement.lang = next;
  if (next === locale) return;
  locale = next;
  subscribers.forEach((listener) => listener());
}

/** English source keys also serve as the fallback, with named interpolation for word order. */
export function translate(language: Locale, key: Message, values: Record<string, string | number> = {}): string {
  const message: string = language === "pt-BR" ? portuguese[key] ?? key : key;
  return message.replace(/\{(\w+)\}/g, (token, name: string) => String(values[name] ?? token));
}
export const t = (key: Message, values?: Record<string, string | number>) => translate(locale, key, values);

const numberFormats = new Map<string, Intl.NumberFormat>();
export function decimal(value: number, digits: number): string {
  const key = `${locale}/${digits}`;
  let formatter = numberFormats.get(key);
  if (!formatter) {
    formatter = new Intl.NumberFormat(locale, { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: false });
    numberFormats.set(key, formatter);
  }
  return formatter.format(value);
}

// Legacy backend diagnostics are English strings. Match complete messages at the presentation
// boundary, preserving raw logs, ids, paths, OCR evidence and external error details. Compile once.
const diagnosticTemplates = Object.keys(portuguese).filter((key) => key.includes("{"))
  .sort((a, b) => b.replace(/\{\w+\}/g, "").length - a.replace(/\{\w+\}/g, "").length).map((key) => {
  const names: string[] = [];
  const pattern = key.split(/(\{\w+\})/g).map((part) => {
    if (/^\{\w+\}$/.test(part)) {
      names.push(part.slice(1, -1));
      return "([\\s\\S]*?)";
    }
    return part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }).join("");
  return { key: key as Message, names, pattern: new RegExp(`^${pattern}$`) };
});

/** Only application messages are translated; unknown technical details remain verbatim. */
export function detail(value: string, depth = 0): string {
  if (locale === "en" || depth > 4) return value;
  if (Object.hasOwn(portuguese, value)) return t(value as Message);
  for (const template of diagnosticTemplates) {
    const match = template.pattern.exec(value);
    if (!match) continue;
    const values = Object.fromEntries(template.names.map((name, i) => [name,
      ["detail", "name", "status", "phase"].includes(name) ? detail(match[i + 1], depth + 1) : match[i + 1],
    ]));
    return t(template.key, values);
  }
  return value;
}
