import { authPage, type AuthSearchParams } from "../auth-page";

export default async function Page({ searchParams }: { searchParams: AuthSearchParams }) {
  return authPage("forgot", searchParams);
}
