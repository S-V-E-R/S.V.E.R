import { router } from "expo-router";
import { HeaderBar, Page } from "../src/ui";
import { BuyValor } from "../src/valor";

/** Your Valor and the store's Valor packs, from the Account tab. */
export default function WalletScreen() {
  return (
    <Page header={<HeaderBar title="Valor" onBack={() => router.back()} />}>
      <BuyValor />
    </Page>
  );
}
