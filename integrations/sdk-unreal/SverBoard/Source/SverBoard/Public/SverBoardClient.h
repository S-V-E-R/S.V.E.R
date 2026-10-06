// S.V.E.R board SDK for Unreal Engine 5 (docs/CROWDSYNC.md "Game SDK").
//
// Create one client (Blueprint: Construct Object from Class, or NewObject in C++), bind its
// events, then Connect with a game token from Creator Studio -> Board -> Connections:
//
//   Board = NewObject<USverBoardClient>(this);
//   Board->OnPress.AddDynamic(this, &AMyGame::HandlePress);   // (Control, Username, Text)
//   Board->OnMove.AddDynamic(this, &AMyGame::HandleMove);     // joystick (Control, Username, X, Y)
//   Board->Connect(Token);
//   Board->SetDisabled(TEXT("jump"), true);                   // game -> board
//
// Events fire on the game thread. It reconnects on its own unless the token was revoked.
#pragma once

#include "CoreMinimal.h"
#include "UObject/Object.h"
#include "SverBoardClient.generated.h"

class IWebSocket;
class FJsonObject;

DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FSverHello, const FString&, Channel);
DECLARE_DYNAMIC_MULTICAST_DELEGATE_ThreeParams(FSverPress, const FString&, Control, const FString&, Username, const FString&, Text);
DECLARE_DYNAMIC_MULTICAST_DELEGATE_FourParams(FSverMove, const FString&, Control, const FString&, Username, float, X, float, Y);
DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FSverJson, const FString&, Json);
DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FSverError, const FString&, Message);

UCLASS(BlueprintType)
class SVERBOARD_API USverBoardClient : public UObject
{
	GENERATED_BODY()

public:
	/** Connected; the channel name. The published board is in LastBoardJson. */
	UPROPERTY(BlueprintAssignable, Category = "S.V.E.R") FSverHello OnHello;
	/** A viewer pressed a button, goal or text input (Text is empty for buttons). */
	UPROPERTY(BlueprintAssignable, Category = "S.V.E.R") FSverPress OnPress;
	/** A viewer moved a joystick (X and Y from -1 to 1). */
	UPROPERTY(BlueprintAssignable, Category = "S.V.E.R") FSverMove OnMove;
	/** A Skill, emote combo or Surge celebration played (the raw event JSON). */
	UPROPERTY(BlueprintAssignable, Category = "S.V.E.R") FSverJson OnEffect;
	/** A new board version was published, the board was paused, or labels, availability or goals changed (raw JSON). */
	UPROPERTY(BlueprintAssignable, Category = "S.V.E.R") FSverJson OnBoardChanged;
	UPROPERTY(BlueprintAssignable, Category = "S.V.E.R") FSverError OnError;

	/** The latest board snapshot as JSON ("board", "version", "state", "goals"). */
	UPROPERTY(BlueprintReadOnly, Category = "S.V.E.R") FString LastBoardJson;

	UFUNCTION(BlueprintCallable, Category = "S.V.E.R")
	void Connect(const FString& Token, const FString& Gateway = TEXT("wss://sver.tv/api/integrations/ws"));
	UFUNCTION(BlueprintCallable, Category = "S.V.E.R")
	void Disconnect();
	UFUNCTION(BlueprintPure, Category = "S.V.E.R")
	bool IsConnected() const;

	/** Changes the board: a JSON object of control changes, e.g. {"jump":{"disabled":true}}. At most 10 a second. */
	UFUNCTION(BlueprintCallable, Category = "S.V.E.R")
	void SetStateJson(const FString& ControlsJson);
	UFUNCTION(BlueprintCallable, Category = "S.V.E.R")
	void SetDisabled(const FString& Control, bool bDisabled);
	/** An empty label restores the label set in the board builder. */
	UFUNCTION(BlueprintCallable, Category = "S.V.E.R")
	void SetLabel(const FString& Control, const FString& Label);
	UFUNCTION(BlueprintCallable, Category = "S.V.E.R")
	void SetProgress(const FString& Control, int32 Progress);

	virtual void BeginDestroy() override;

private:
	void Open();
	void Handle(const FString& Message);
	void Send(const TSharedRef<FJsonObject>& Message);
	void SendControl(const FString& Control, const TSharedRef<FJsonObject>& Change);

	TSharedPtr<IWebSocket> Socket;
	FString Token;
	FString Gateway;
	bool bStopped = true;
	float RetrySeconds = 1.f;
	int32 NextId = 1;
};
