import assert from "node:assert/strict";
import test from "node:test";
import { isExpoPushToken, notificationTarget } from "./push.ts";

test("expo push tokens keep the Expo shape", () => {
  assert.equal(isExpoPushToken("ExponentPushToken[alertfana000000000001]"), true);
  assert.equal(isExpoPushToken("ExpoPushToken[alertfana000000000001]"), true);
  assert.equal(isExpoPushToken("https://exp.host/--/api/v2/push/send"), false);
  assert.equal(isExpoPushToken("ExponentPushToken[short]"), false);
});

test("a go-live path opens the channel, and other paths stay on the website", () => {
  assert.deepEqual(notificationTarget("/AlertOwner"), { screen: "watch", username: "AlertOwner" });
  assert.deepEqual(notificationTarget("/messages?with=AlertFanH"), { screen: "web", path: "/messages?with=AlertFanH" });
  assert.deepEqual(notificationTarget("/g/northwind"), { screen: "web", path: "/g/northwind" });
  assert.equal(notificationTarget("https://evil.example/AlertOwner"), null);
  assert.equal(notificationTarget("//evil.example"), null);
});
