/** Stream languages (ISO 639-1) and "other"; the API's list is streams::LANGUAGES. */
export const LANGUAGES: [string, string][] = [
  ["ar", "Arabic"], ["bg", "Bulgarian"], ["bn", "Bengali"], ["cs", "Czech"], ["da", "Danish"], ["de", "German"],
  ["el", "Greek"], ["en", "English"], ["es", "Spanish"], ["et", "Estonian"], ["fa", "Persian"], ["fi", "Finnish"],
  ["fr", "French"], ["he", "Hebrew"], ["hi", "Hindi"], ["hr", "Croatian"], ["hu", "Hungarian"], ["id", "Indonesian"],
  ["it", "Italian"], ["ja", "Japanese"], ["ko", "Korean"], ["lt", "Lithuanian"], ["lv", "Latvian"], ["ms", "Malay"],
  ["nl", "Dutch"], ["no", "Norwegian"], ["pl", "Polish"], ["pt", "Portuguese"], ["ro", "Romanian"], ["ru", "Russian"],
  ["sk", "Slovak"], ["sr", "Serbian"], ["sv", "Swedish"], ["th", "Thai"], ["tl", "Filipino"], ["tr", "Turkish"],
  ["uk", "Ukrainian"], ["ur", "Urdu"], ["vi", "Vietnamese"], ["zh", "Chinese"], ["other", "Other"],
];
const CODES = new Set(LANGUAGES.map(([code]) => code));
export const languageName = (code: string) => LANGUAGES.find(([c]) => c === code)?.[1] ?? code;

/** Supported codes from a browser's languages ("en-US", "pt-BR" or an Accept-Language header). */
export function fromBrowser(tags: readonly string[] | string | null | undefined): string[] {
  const list = typeof tags === "string" ? tags.split(",").map(t => t.split(";")[0]) : tags ?? [];
  return [...new Set(list.map(t => t.trim().slice(0, 2).toLowerCase()).filter(c => CODES.has(c)))];
}
