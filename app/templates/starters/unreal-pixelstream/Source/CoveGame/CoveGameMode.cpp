#include "CoveGameMode.h"

#include "Components/DirectionalLightComponent.h"
#include "Components/SkyAtmosphereComponent.h"
#include "Components/SkyLightComponent.h"
#include "Components/StaticMeshComponent.h"
#include "Engine/DirectionalLight.h"
#include "Engine/ExponentialHeightFog.h"
#include "Engine/SkyLight.h"
#include "Engine/StaticMesh.h"
#include "Engine/StaticMeshActor.h"
#include "GameFramework/DefaultPawn.h"
#include "GameFramework/RotatingMovementComponent.h"
#include "Materials/MaterialInstanceDynamic.h"

ACoveGameMode::ACoveGameMode()
{
	// WASD and the mouse fly it, from a keyboard or from a browser.
	DefaultPawnClass = ADefaultPawn::StaticClass();
}

void ACoveGameMode::BeginPlay()
{
	Super::BeginPlay();
	UWorld* World = GetWorld();
	if (!World)
	{
		return;
	}

	// The sun, low enough for long shadows; movable, as nothing is baked.
	ADirectionalLight* Sun = World->SpawnActor<ADirectionalLight>(FVector::ZeroVector, FRotator(-35.f, 40.f, 0.f));
	if (Sun)
	{
		UDirectionalLightComponent* Light = Cast<UDirectionalLightComponent>(Sun->GetLightComponent());
		Light->SetMobility(EComponentMobility::Movable);
		Light->SetIntensity(8.f);
		Light->SetAtmosphereSunLight(true);
	}
	World->SpawnActor<ASkyAtmosphere>(FVector::ZeroVector, FRotator::ZeroRotator);
	if (ASkyLight* Sky = World->SpawnActor<ASkyLight>(FVector::ZeroVector, FRotator::ZeroRotator))
	{
		USkyLightComponent* Light = Sky->GetLightComponent();
		Light->SetMobility(EComponentMobility::Movable);
		Light->bRealTimeCapture = true;
		Light->RecaptureSky();
	}
	World->SpawnActor<AExponentialHeightFog>(FVector::ZeroVector, FRotator::ZeroRotator);

	// The floor, then a ring of shapes turning at their own speeds.
	SpawnShape(TEXT("/Engine/BasicShapes/Plane.Plane"), FVector(0.f, 0.f, 0.f), FVector(60.f, 60.f, 1.f), FLinearColor(0.08f, 0.09f, 0.11f), 0.f);
	const TCHAR* Shapes[] = {TEXT("/Engine/BasicShapes/Cube.Cube"), TEXT("/Engine/BasicShapes/Sphere.Sphere"), TEXT("/Engine/BasicShapes/Cone.Cone"), TEXT("/Engine/BasicShapes/Cylinder.Cylinder")};
	const int32 Count = 12;
	for (int32 i = 0; i < Count; ++i)
	{
		const float Angle = 2.f * PI * i / Count;
		const FVector Where(FMath::Cos(Angle) * 900.f, FMath::Sin(Angle) * 900.f, 150.f + 60.f * (i % 3));
		const FLinearColor Color = FLinearColor::MakeFromHSV8(static_cast<uint8>(255 * i / Count), 170, 230);
		SpawnShape(Shapes[i % 4], Where, FVector(1.5f), Color, 20.f + 15.f * (i % 4));
	}
}

AActor* ACoveGameMode::SpawnShape(const TCHAR* Mesh, const FVector& Where, const FVector& Scale, const FLinearColor& Color, float SpinDegreesPerSecond)
{
	UStaticMesh* StaticMesh = LoadObject<UStaticMesh>(nullptr, Mesh);
	AStaticMeshActor* Actor = StaticMesh ? GetWorld()->SpawnActor<AStaticMeshActor>(Where, FRotator::ZeroRotator) : nullptr;
	if (!Actor)
	{
		return nullptr;
	}
	UStaticMeshComponent* Component = Actor->GetStaticMeshComponent();
	Component->SetMobility(EComponentMobility::Movable);
	Component->SetStaticMesh(StaticMesh);
	Actor->SetActorScale3D(Scale);
	// BasicShapeMaterial takes its colour from a "Color" parameter.
	if (UMaterialInterface* Base = LoadObject<UMaterialInterface>(nullptr, TEXT("/Engine/BasicShapes/BasicShapeMaterial.BasicShapeMaterial")))
	{
		UMaterialInstanceDynamic* Tint = UMaterialInstanceDynamic::Create(Base, Actor);
		Tint->SetVectorParameterValue(TEXT("Color"), Color);
		Component->SetMaterial(0, Tint);
	}
	if (SpinDegreesPerSecond != 0.f)
	{
		URotatingMovementComponent* Spin = NewObject<URotatingMovementComponent>(Actor);
		Spin->RotationRate = FRotator(0.f, SpinDegreesPerSecond, 0.f);
		Spin->SetUpdatedComponent(Component);
		Spin->RegisterComponent();
	}
	return Actor;
}
