// Crowd Runner for Unreal: the browser example game (integrations/example-game) on the SverBoard
// plugin. Viewers make the runner jump, change its speed with a joystick and shout; the game
// disables Jump while the runner is airborne and fills the coins goal as it runs.
//
// Board controls: a button "jump", a goal "coins", a text input "shout", a joystick "move", and
// optionally a "Game confirms" button "boost" (captured when the boost happens, else released).
//
// ACrowdRunnerActor plays it in a level; UCrowdRunnerCommandlet runs it headless:
//   UnrealEditor-Cmd CrowdRunner.uproject -run=CrowdRunner -token=<game token> [-gateway=<url>] [-seconds=60]
#pragma once

#include "CoreMinimal.h"
#include "UObject/Object.h"
#include "CrowdRunnerGame.generated.h"

class USverBoardClient;

DECLARE_LOG_CATEGORY_EXTERN(LogCrowdRunner, Log, All);

UCLASS()
class CROWDRUNNER_API UCrowdRunnerGame : public UObject
{
	GENERATED_BODY()

public:
	/** Connects to the board; an empty token plays locally. */
	void Start(const FString& Token, const FString& Gateway);
	void Tick(float DeltaSeconds);
	void Jump();

	float Height = 0.f;
	float Speed = 4.f;
	int32 Collected = 0;
	FString Shout;

private:
	UFUNCTION() void HandleHello(const FString& Channel);
	UFUNCTION() void HandlePress(const FString& Control, const FString& Username, const FString& Text, const FString& PressId, bool bConfirm);
	UFUNCTION() void HandleMove(const FString& Control, const FString& Username, float X, float Y);
	UFUNCTION() void HandleResult(const FString& PressId, const FString& Control, const FString& Outcome);
	UFUNCTION() void HandleError(const FString& Message);
	void SyncJump();

	UPROPERTY() TObjectPtr<USverBoardClient> Board;
	float Rise = 0.f;
	float Distance = 0.f;
	float Clock = 0.f;
	float BoostUntil = 0.f;
	float ShoutUntil = 0.f;
	bool bAirborne = false;
};
