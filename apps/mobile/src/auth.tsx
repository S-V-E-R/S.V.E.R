import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { send, type Result } from "./api";
import { clearJar } from "./session";
import { palette, themeFor, type Colors, type ThemeName } from "./theme";
import type { Account, LoginBody } from "./types";

type AuthValue = {
  ready: boolean;
  account: Account | null;
  theme: ThemeName;
  colors: Colors;
  refresh: () => Promise<void>;
  signIn: (identifier: string, password: string) => Promise<Result<LoginBody>>;
  confirmMfa: (code: string) => Promise<Result<LoginBody>>;
  signOut: () => Promise<void>;
};

const AuthContext = createContext<AuthValue | null>(null);

function accountFrom(data: {
  id?: unknown;
  username?: unknown;
  faction?: unknown;
  email_verified?: unknown;
  deletion_due?: unknown;
}): Account | null {
  if (typeof data.id !== "string" || typeof data.username !== "string") return null;
  return {
    id: data.id,
    username: data.username,
    faction: typeof data.faction === "string" ? data.faction : null,
    email_verified: data.email_verified === true,
    deletion_due: typeof data.deletion_due === "string" ? data.deletion_due : null,
  };
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [account, setAccount] = useState<Account | null>(null);

  const refresh = useCallback(async () => {
    const result = await send<Parameters<typeof accountFrom>[0]>("GET", "/api/auth/me");
    if (result.ok) setAccount(accountFrom(result.data));
    else {
      setAccount(null);
      if (result.status === 401) await clearJar();
    }
  }, []);

  useEffect(() => {
    // The account arrives after the request. The rule treats the call itself as a render update.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh().finally(() => setReady(true));
  }, [refresh]);

  const finish = useCallback(async (result: Result<LoginBody>) => {
    if (result.ok && result.data.signed_in) await refresh();
    return result;
  }, [refresh]);

  const signIn = useCallback(async (identifier: string, password: string) => {
    const result = await send<LoginBody>("POST", "/api/auth/login", { identifier, password });
    return finish(result);
  }, [finish]);

  const confirmMfa = useCallback(async (code: string) => {
    const result = await send<LoginBody>("POST", "/api/auth/mfa/login", { code });
    return finish(result);
  }, [finish]);

  const signOut = useCallback(async () => {
    await send("POST", "/api/auth/logout");
    await clearJar();
    setAccount(null);
  }, []);

  const theme = themeFor(account);
  const value = useMemo<AuthValue>(() => ({
    ready, account, theme, colors: palette[theme], refresh, signIn, confirmMfa, signOut,
  }), [ready, account, theme, refresh, signIn, confirmMfa, signOut]);

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthValue {
  const value = useContext(AuthContext);
  if (!value) throw new Error("useAuth must be used inside AuthProvider");
  return value;
}
