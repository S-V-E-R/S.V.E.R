#include "CrowdRunnerGame.h"

#include "SverBoardClient.h"

DEFINE_LOG_CATEGORY(LogCrowdRunner);

namespace
{
	constexpr float Gravity = -30.f;
	constexpr float JumpSpeed = 12.f;
}

void UCrowdRunnerGame::Start(const FString& Token, const FString& Gateway)
{
	if (Token.IsEmpty())
	{
		UE_LOG(LogCrowdRunner, Display, TEXT("No token: playing locally."));
		return;
	}
	Board = NewObject<USverBoardClient>(this);
	Board->OnHello.AddDynamic(this, &UCrowdRunnerGame::HandleHello);
	Board->OnPress.AddDynamic(this, &UCrowdRunnerGame::HandlePress);
	Board->OnMove.AddDynamic(this, &UCrowdRunnerGame::HandleMove);
	Board->OnResult.AddDynamic(this, &UCrowdRunnerGame::HandleResult);
	Board->OnError.AddDynamic(this, &UCrowdRunnerGame::HandleError);
	if (Gateway.IsEmpty())
	{
		Board->Connect(Token);
	}
	else
	{
		Board->Connect(Token, Gateway);
	}
}

void UCrowdRunnerGame::HandleHello(const FString& Channel)
{
	UE_LOG(LogCrowdRunner, Display, TEXT("Connected to %s's board."), *Channel);
	Board->Ready();
	Board->SetProgress(TEXT("coins"), FMath::Min(Collected, 10));
}

void UCrowdRunnerGame::HandlePress(const FString& Control, const FString& Username, const FString& Text, const FString& PressId, bool bConfirm)
{
	UE_LOG(LogCrowdRunner, Display, TEXT("Press %s by %s%s%s"), *Control, *Username, Text.IsEmpty() ? TEXT("") : TEXT(": "), *Text);
	if (Control == TEXT("jump"))
	{
		Jump();
	}
	else if (Control == TEXT("shout") && !Text.IsEmpty())
	{
		Shout = FString::Printf(TEXT("%s: %s"), *Username, *Text);
		ShoutUntil = Clock + 4.f;
	}
	else if (Control == TEXT("boost"))
	{
		// A "Game confirms" control: charge the viewer only if the boost happened.
		if (bConfirm && Clock < BoostUntil)
		{
			Board->Release(PressId);
			return;
		}
		BoostUntil = Clock + 3.f;
		if (bConfirm)
		{
			Board->Capture(PressId);
		}
	}
}

void UCrowdRunnerGame::HandleMove(const FString& Control, const FString& Username, float X, float Y)
{
	if (Control != TEXT("move"))
	{
		return;
	}
	Speed = FMath::Clamp(Speed + X, 2.f, 9.f);
	UE_LOG(LogCrowdRunner, Display, TEXT("%s set the speed to %.1f."), *Username, Speed);
}

void UCrowdRunnerGame::HandleResult(const FString& PressId, const FString& Control, const FString& Outcome)
{
	UE_LOG(LogCrowdRunner, Display, TEXT("Press %s %s"), *PressId, *Outcome);
}

void UCrowdRunnerGame::HandleError(const FString& Message)
{
	UE_LOG(LogCrowdRunner, Warning, TEXT("Board error: %s"), *Message);
}

void UCrowdRunnerGame::Jump()
{
	if (bAirborne)
	{
		return;
	}
	bAirborne = true;
	Rise = JumpSpeed;
	SyncJump();
	UE_LOG(LogCrowdRunner, Display, TEXT("Jumped."));
}

// Viewers can't press Jump while the runner is in the air.
void UCrowdRunnerGame::SyncJump()
{
	if (Board && Board->IsConnected())
	{
		Board->SetStateJson(bAirborne
			? TEXT("{\"jump\":{\"disabled\":true,\"label\":\"Jump (in the air)\"}}")
			: TEXT("{\"jump\":{\"disabled\":false,\"label\":null}}"));
	}
}

void UCrowdRunnerGame::Tick(float DeltaSeconds)
{
	Clock += DeltaSeconds;
	Distance += Speed * (Clock < BoostUntil ? 2.f : 1.f) * DeltaSeconds;
	if (bAirborne)
	{
		Rise += Gravity * DeltaSeconds;
		Height = FMath::Max(0.f, Height + Rise * DeltaSeconds);
		if (Height == 0.f)
		{
			bAirborne = false;
			SyncJump();
			UE_LOG(LogCrowdRunner, Display, TEXT("Landed."));
		}
	}
	const int32 Coins = static_cast<int32>(Distance / 10.f);
	if (Coins != Collected)
	{
		Collected = Coins;
		if (Board && Board->IsConnected())
		{
			Board->SetProgress(TEXT("coins"), FMath::Min(Collected, 10));
		}
	}
	if (Clock > ShoutUntil)
	{
		Shout.Reset();
	}
}
