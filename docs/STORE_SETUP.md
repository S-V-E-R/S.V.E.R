# Store setup for buying Valor in the app

Joe's checklist for selling Valor packs through the App Store and Google Play (decided November 18, 2026; `docs/MOBILE.md`). Only the account owner can do these steps. The rules, prices and refunds are in `docs/SUPPORT.md` "Store packs", and testing on a phone is in `apps/mobile/README.md` "Buying Valor on a device".

Secrets go only in the server's private environment, never in the repo, a commit, a ticket or a chat. Below, `<secret>` stands for the value of `STORE_NOTIFY_SECRET`.

## 1. Server

1. Make a long random value for `STORE_NOTIFY_SECRET` (for example `openssl rand -hex 32`) and set it on the API host. The notification addresses below end with it.
2. Set `STORE_TEST_USERS` to the S.V.E.R user ids of the App Review demo account and the testers, comma-separated. Use only accounts with no ties to any creator: test Valor is free but pays creators when it is spent.
3. Leave `STORE_TEST_MONTHLY_VALOR` unset for the default of 2,000 Valor per test account a month, or set another limit.
4. Leave `STORE_GOOGLE_US_LINK` unset (or `false`) until section 4 below is done.
5. The Apple and Google keys come from the steps below. Restart the API after setting them; it refuses to start if a key is set but broken.

## 2. App Store Connect

1. Confirm the Paid Apps agreement, tax forms and banking are active.
2. Enroll in the **App Store Small Business Program**. Without it Apple's fee is 30%, and every 100 Valor spent loses about 11¢.
3. Create 7 **Consumable** in-app purchases:

   | Product id | Price | Display name |
   | --- | --- | --- |
   | `tv.sver.app.valor.100` | $0.99 | 100 Valor |
   | `tv.sver.app.valor.200` | $1.99 | 200 Valor |
   | `tv.sver.app.valor.500` | $4.99 | 500 Valor |
   | `tv.sver.app.valor.1000` | $9.99 | 1,000 Valor |
   | `tv.sver.app.valor.2000` | $19.99 | 2,000 Valor |
   | `tv.sver.app.valor.5000` | $49.99 | 5,000 Valor |
   | `tv.sver.app.valor.10000` | $99.99 | 10,000 Valor |

   Add a review screenshot to each. The first in-app purchases go to review together with an app version.
4. **Storefronts:** in each purchase's availability, choose the US and storefronts with no VAT or low tax only (`docs/SUPPORT.md` "Store packs", economics). Adding a VAT storefront later loses money on every pack unless its price changes.
5. Under Users and Access → Integrations → **In-App Purchase**, create a key and download the `.p8` file (it downloads only once). On the API host set:
   - `APPLE_IAP_ISSUER_ID`: the issuer id shown on that page
   - `APPLE_IAP_KEY_ID`: the key's id
   - `APPLE_IAP_PRIVATE_KEY`: the contents of the `.p8` file
6. Under App Information → **App Store Server Notifications**, choose version 2 and set both the Production and the Sandbox address to `https://sver.tv/api/store/apple/notifications/<secret>`. Apple sends its test notification only through its API, not from this page, so the test purchase and refund in section 5 check this address instead.
7. Under Users and Access → Sandbox, create sandbox testers for the testers' phones.
8. In the App Review information for the release, give the demo account's sign-in. Its user id must be in `STORE_TEST_USERS`, because App Review buys in the sandbox against the production server.
9. **US link:** Apple needs no extra permission for the "Bonus Valor packs are on sver.tv." line in the US storefront. Before the release, read Apple's current US terms for links to purchases outside the app, in case they changed.

## 3. Play Console and Google Cloud

1. Set up the payments profile.
2. Check that the 15% service fee on the first $1M of earnings a year applies to the account; Play Console may ask for an account group to enroll.
3. Upload a build to the **internal testing** track. Google Play needs a build with billing before it lets you create products, and it answers purchases only for a build it knows.
4. Create 7 **one-time products** with the same ids and prices as the table above, and activate them.
5. **Storefronts:** make the products available in the US and in countries with no VAT or low tax only, the same list as on Apple.
6. In Google Cloud, enable the **Google Play Android Developer API**, create a service account, and create a JSON key for it. Set `GOOGLE_PLAY_SERVICE_ACCOUNT` on the API host to the contents of that file.
7. In Play Console → Users and permissions, invite the service account's email with **View financial data** and **Manage orders and subscriptions**.
8. Real-time notifications:
   - Create a Pub/Sub topic.
   - Give `google-play-developer-notifications@system.gserviceaccount.com` the **Pub/Sub Publisher** role on it.
   - Add a push subscription to the topic with the address `https://sver.tv/api/store/google/notifications/<secret>`.
   - In Play Console → Monetization setup, choose that topic and **all notifications for subscriptions and one-time products**.
   - Send a test notification from that page.
9. Add the testers' Google accounts as **license testers**, and their S.V.E.R user ids to `STORE_TEST_USERS`.

## 4. Google's US program for the link to sver.tv

The app can show "Bonus Valor packs are on sver.tv." to US Android users only through Google's US program for links to content outside the app.

1. Read the program's current terms and fees in Play Console, and enroll the app for the US if you accept them.
2. The program also requires calls from the app itself: checking that the link is allowed, showing Google's information screen before the browser opens, and reporting purchases that come from the link. `apps/mobile/README.md` says whether this build makes them.
3. Only when both are done, set `STORE_GOOGLE_US_LINK=true` on the API host. Until then Android never shows the line. iOS shows it in the US storefront without this switch.

## 5. Builds and the first real test

1. From `apps/mobile`, run `npx eas-cli init` to create the EAS project.
2. Make an iOS build (TestFlight or ad hoc) and an Android build on internal testing.
3. On each, with a test account, buy the $0.99 pack and confirm 100 Valor arrives on that account.
4. Refund one test purchase where the store allows it (Play Console's order management can refund a license tester's order) and confirm its Valor is removed.
5. Check the staff money page: the purchases and the refund are listed.
6. Then submit the app version together with its in-app purchases.
