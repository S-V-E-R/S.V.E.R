// Runs Crowd Runner headless for a set time, for testing a board without opening a level:
//   UnrealEditor-Cmd CrowdRunner.uproject -run=CrowdRunner -token=<game token> [-gateway=<url>] [-seconds=60]
#pragma once

#include "CoreMinimal.h"
#include "Commandlets/Commandlet.h"
#include "CrowdRunnerCommandlet.generated.h"

UCLASS()
class CROWDRUNNER_API UCrowdRunnerCommandlet : public UCommandlet
{
	GENERATED_BODY()

public:
	virtual int32 Main(const FString& Params) override;
};
