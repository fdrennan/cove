// The game mode: a flying camera (ADefaultPawn) and a scene made in code at
// the start - a sun, a sky, a floor and a ring of turning shapes - so the
// project needs no assets of its own to show something on its first run.
#pragma once

#include "CoreMinimal.h"
#include "GameFramework/GameModeBase.h"
#include "CoveGameMode.generated.h"

UCLASS()
class COVEGAME_API ACoveGameMode : public AGameModeBase
{
	GENERATED_BODY()

public:
	ACoveGameMode();

protected:
	virtual void BeginPlay() override;

private:
	/** A mesh from the engine's basic shapes, placed, scaled and tinted. */
	AActor* SpawnShape(const TCHAR* Mesh, const FVector& Where, const FVector& Scale, const FLinearColor& Color, float SpinDegreesPerSecond);
};
