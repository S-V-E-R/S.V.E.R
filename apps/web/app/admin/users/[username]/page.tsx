import UserStanding from "./standing";
export default async function Page({ params }: { params: Promise<{ username: string }> }) {
  return <UserStanding username={(await params).username} />;
}
