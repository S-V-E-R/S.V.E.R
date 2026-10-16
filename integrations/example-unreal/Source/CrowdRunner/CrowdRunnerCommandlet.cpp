#include "CrowdRunnerCommandlet.h"

#include "Async/TaskGraphInterfaces.h"
#include "Containers/Ticker.h"
#include "CrowdRunnerGame.h"
#include "HAL/PlatformProcess.h"
#include "HttpModule.h"
#include "HttpManager.h"

int32 UCrowdRunnerCommandlet::Main(const FString& Params)
{
	FString Token, Gateway;
	float Seconds = 60.f;
	FParse::Value(*Params, TEXT("token="), Token);
	FParse::Value(*Params, TEXT("gateway="), Gateway);
	FParse::Value(*Params, TEXT("seconds="), Seconds);
	if (Token.IsEmpty())
	{
		UE_LOG(LogCrowdRunner, Error, TEXT("Pass -token=<game token>."));
		return 1;
	}
	UCrowdRunnerGame* Game = NewObject<UCrowdRunnerGame>();
	Game->AddToRoot();
	Game->Start(Token, Gateway);
	// No engine loop runs in a commandlet, so pump the tickers that deliver socket events.
	constexpr float Step = 1.f / 60.f;
	for (float Elapsed = 0.f; Elapsed < Seconds && !IsEngineExitRequested(); Elapsed += Step)
	{
		FTaskGraphInterface::Get().ProcessThreadUntilIdle(ENamedThreads::GameThread);
		FHttpModule::Get().GetHttpManager().Tick(Step);
		FTSTicker::GetCoreTicker().Tick(Step);
		Game->Tick(Step);
		FPlatformProcess::Sleep(Step);
	}
	Game->RemoveFromRoot();
	return 0;
}
