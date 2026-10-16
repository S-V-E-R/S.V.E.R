#include "CrowdRunnerActor.h"

#include "Components/StaticMeshComponent.h"
#include "CrowdRunnerGame.h"
#include "Engine/Engine.h"
#include "Misc/CommandLine.h"
#include "UObject/ConstructorHelpers.h"

ACrowdRunnerActor::ACrowdRunnerActor()
{
	PrimaryActorTick.bCanEverTick = true;
	Runner = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Runner"));
	RootComponent = Runner;
	static ConstructorHelpers::FObjectFinder<UStaticMesh> Cube(TEXT("/Engine/BasicShapes/Cube.Cube"));
	if (Cube.Succeeded())
	{
		Runner->SetStaticMesh(Cube.Object);
	}
}

void ACrowdRunnerActor::BeginPlay()
{
	Super::BeginPlay();
	FString Token, Gateway;
	FParse::Value(FCommandLine::Get(), TEXT("token="), Token);
	FParse::Value(FCommandLine::Get(), TEXT("gateway="), Gateway);
	Game = NewObject<UCrowdRunnerGame>(this);
	Game->Start(Token, Gateway);
}

void ACrowdRunnerActor::Tick(float DeltaSeconds)
{
	Super::Tick(DeltaSeconds);
	Game->Tick(DeltaSeconds);
	// 100 units per metre.
	Runner->SetRelativeLocation(FVector(0.f, 0.f, Game->Height * 100.f));
	if (GEngine)
	{
		GEngine->AddOnScreenDebugMessage(1, 0.f, FColor::White, FString::Printf(TEXT("Coins %d  Speed %.1f"), Game->Collected, Game->Speed));
		if (!Game->Shout.IsEmpty())
		{
			GEngine->AddOnScreenDebugMessage(2, 0.f, FColor::Yellow, Game->Shout);
		}
	}
}
