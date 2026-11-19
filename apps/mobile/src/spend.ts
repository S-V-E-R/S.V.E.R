/**
 * Spending Valor from the app: tributes, Skills, Valor months and gift subs (docs/MOBILE.md).
 * Pure helpers, so `node --test` runs them without React Native.
 */

/** No answer, a server failure or a rate limit: the action may or may not have gone through. */
export const unanswered = (status: number) => status === 0 || status === 429 || status >= 500;

/**
 * One request id per money action. A lost response (status 0, 5xx, 429) keeps the id, so trying
 * again replays the same request and the server charges it once. A definitive answer, or a change
 * to what is being bought (`key`: amount, tier, count, recipient or Skill), starts a new id.
 */
export class Attempt {
  private id = "";
  private key = "";
  /** True when `take` handed back the id of an earlier unanswered try. */
  reused = false;
  private readonly make: () => string;
  constructor(make: () => string) {
    this.make = make;
  }

  take(key: string): string {
    this.reused = !!this.id && key === this.key;
    if (!this.reused) {
      this.id = this.make();
      this.key = key;
    }
    return this.id;
  }

  settle(result: { ok: boolean; status?: number }): void {
    if (result.ok || !unanswered(result.status ?? 0)) this.id = "";
  }
}

/** The tribute field: a whole number of Valor, at least 10. Null when it can't be paid. */
export function parseTribute(text: string): number | null {
  const n = Math.floor(Number(text));
  return Number.isFinite(n) && n >= 10 ? n : null;
}

/** A refusal about the viewer's Valor gets a "Get Valor" next to it, as on the website. */
export const aboutValor = (error: string) => /\bValor\b/.test(error);
