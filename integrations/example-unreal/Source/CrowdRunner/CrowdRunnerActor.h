// Drop into any level: a cube runner driven by the board. The token comes from -token=<game
// token> on the command line (keep it out of the project) and the gateway from -gateway=<url>.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "CrowdRunnerActor.generated.h"

class UCrowdRunnerGame;
class UStaticMeshComponent;

UCLASS()
class CROWDRUNNER_API ACrowdRunnerActor : public AActor
{
	GENERATED_BODY()

public:
	ACrowdRunnerActor();
	virtual void BeginPlay() override;
	virtual void Tick(float DeltaSeconds) override;

private:
	UPROPERTY(VisibleAnywhere) TObjectPtr<UStaticMeshComponent> Runner;
	UPROPERTY() TObjectPtr<UCrowdRunnerGame> Game;
};
