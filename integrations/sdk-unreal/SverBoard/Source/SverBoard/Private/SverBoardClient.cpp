// S.V.E.R board SDK for Unreal Engine 5 (docs/CROWDSYNC.md "Game SDK").
#include "SverBoardClient.h"

#include "Containers/Ticker.h"
#include "Dom/JsonObject.h"
#include "Dom/JsonValue.h"
#include "IWebSocket.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"
#include "Serialization/JsonWriter.h"
#include "WebSocketsModule.h"

namespace
{
	/** Close code the gateway uses when the token was revoked in Creator Studio. */
	constexpr int32 Revoked = 4001;

	FString Write(const TSharedRef<FJsonObject>& Object)
	{
		FString Out;
		const TSharedRef<TJsonWriter<TCHAR, TCondensedJsonPrintPolicy<TCHAR>>> Writer =
			TJsonWriterFactory<TCHAR, TCondensedJsonPrintPolicy<TCHAR>>::Create(&Out);
		FJsonSerializer::Serialize(Object, Writer);
		return Out;
	}
}

void USverBoardClient::Connect(const FString& InToken, const FString& InGateway)
{
	Token = InToken;
	Gateway = InGateway;
	bStopped = false;
	RetrySeconds = 1.f;
	Open();
}

void USverBoardClient::Open()
{
	if (bStopped)
	{
		return;
	}
	FModuleManager::LoadModuleChecked<FWebSocketsModule>(TEXT("WebSockets"));
	TMap<FString, FString> Headers;
	Headers.Add(TEXT("Authorization"), FString::Printf(TEXT("Bearer %s"), *Token));
	Socket = FWebSocketsModule::Get().CreateWebSocket(Gateway, FString(), Headers);

	TWeakObjectPtr<USverBoardClient> Self(this);
	Socket->OnMessage().AddLambda([Self](const FString& Message)
	{
		if (Self.IsValid())
		{
			Self->Handle(Message);
		}
	});
	Socket->OnConnectionError().AddLambda([Self](const FString& Error)
	{
		if (Self.IsValid())
		{
			Self->OnError.Broadcast(Error);
		}
	});
	Socket->OnClosed().AddLambda([Self](int32 Code, const FString& Reason, bool bClean)
	{
		if (!Self.IsValid() || Self->bStopped)
		{
			return;
		}
		if (Code == Revoked)
		{
			Self->bStopped = true;
			Self->OnError.Broadcast(TEXT("The token was revoked in Creator Studio."));
			return;
		}
		// Reconnect with doubling backoff, up to 30 seconds.
		const float Delay = Self->RetrySeconds;
		Self->RetrySeconds = FMath::Min(Self->RetrySeconds * 2.f, 30.f);
		FTSTicker::GetCoreTicker().AddTicker(FTickerDelegate::CreateLambda([Self](float)
		{
			if (Self.IsValid())
			{
				Self->Open();
			}
			return false;
		}), Delay);
	});
	Socket->Connect();
}

void USverBoardClient::Disconnect()
{
	bStopped = true;
	if (Socket.IsValid())
	{
		Socket->Close();
		Socket.Reset();
	}
}

bool USverBoardClient::IsConnected() const
{
	return Socket.IsValid() && Socket->IsConnected();
}

void USverBoardClient::Handle(const FString& Message)
{
	TSharedPtr<FJsonObject> Event;
	if (!FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Message), Event) || !Event.IsValid())
	{
		return;
	}
	const FString Type = Event->GetStringField(TEXT("type"));
	const TSharedPtr<FJsonObject>* User = nullptr;
	FString Username;
	if (Event->TryGetObjectField(TEXT("user"), User) && User)
	{
		(*User)->TryGetStringField(TEXT("username"), Username);
	}
	if (Type == TEXT("hello"))
	{
		RetrySeconds = 1.f;
		LastBoardJson = Message;
		OnHello.Broadcast(Event->GetStringField(TEXT("channel")));
	}
	else if (Type == TEXT("board_effect"))
	{
		FString Control;
		if (Event->TryGetStringField(TEXT("control"), Control))
		{
			FString Text;
			Event->TryGetStringField(TEXT("text"), Text);
			OnPress.Broadcast(Control, Username, Text);
		}
		else
		{
			OnEffect.Broadcast(Message);
		}
	}
	else if (Type == TEXT("board_input"))
	{
		OnMove.Broadcast(Event->GetStringField(TEXT("control")), Username,
			static_cast<float>(Event->GetNumberField(TEXT("x"))), static_cast<float>(Event->GetNumberField(TEXT("y"))));
	}
	else if (Type == TEXT("board") || Type == TEXT("board_state"))
	{
		if (Type == TEXT("board"))
		{
			LastBoardJson = Message;
		}
		OnBoardChanged.Broadcast(Message);
	}
	else if (Type == TEXT("error"))
	{
		OnError.Broadcast(Event->GetStringField(TEXT("message")));
	}
}

void USverBoardClient::Send(const TSharedRef<FJsonObject>& Message)
{
	if (!IsConnected())
	{
		OnError.Broadcast(TEXT("Not connected to S.V.E.R."));
		return;
	}
	Socket->Send(Write(Message));
}

void USverBoardClient::SetStateJson(const FString& ControlsJson)
{
	TSharedPtr<FJsonObject> Controls;
	if (!FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(ControlsJson), Controls) || !Controls.IsValid())
	{
		OnError.Broadcast(TEXT("SetStateJson needs a JSON object of control changes."));
		return;
	}
	const TSharedRef<FJsonObject> Message = MakeShared<FJsonObject>();
	Message->SetStringField(TEXT("type"), TEXT("state"));
	Message->SetNumberField(TEXT("id"), NextId++);
	Message->SetObjectField(TEXT("controls"), Controls);
	Send(Message);
}

void USverBoardClient::SendControl(const FString& Control, const TSharedRef<FJsonObject>& Change)
{
	const TSharedRef<FJsonObject> Controls = MakeShared<FJsonObject>();
	Controls->SetObjectField(Control, Change);
	const TSharedRef<FJsonObject> Message = MakeShared<FJsonObject>();
	Message->SetStringField(TEXT("type"), TEXT("state"));
	Message->SetNumberField(TEXT("id"), NextId++);
	Message->SetObjectField(TEXT("controls"), Controls);
	Send(Message);
}

void USverBoardClient::Ready()
{
	const TSharedRef<FJsonObject> Message = MakeShared<FJsonObject>();
	Message->SetStringField(TEXT("type"), TEXT("ready"));
	Message->SetNumberField(TEXT("id"), NextId++);
	Send(Message);
}

static TSharedRef<FJsonObject> Settle(const TCHAR* Type, const FString& Press, int32 Id)
{
	const TSharedRef<FJsonObject> Message = MakeShared<FJsonObject>();
	Message->SetStringField(TEXT("type"), Type);
	Message->SetNumberField(TEXT("id"), Id);
	Message->SetStringField(TEXT("press"), Press);
	return Message;
}

void USverBoardClient::SetGroupsJson(const FString& By, const FString& ScreensJson)
{
	const TSharedRef<FJsonObject> Message = MakeShared<FJsonObject>();
	Message->SetStringField(TEXT("type"), TEXT("groups"));
	Message->SetNumberField(TEXT("id"), NextId++);
	if (By.IsEmpty())
	{
		Message->SetField(TEXT("by"), MakeShared<FJsonValueNull>());
	}
	else
	{
		TSharedPtr<FJsonValue> Screens;
		if (!FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(ScreensJson), Screens) || !Screens.IsValid())
		{
			OnError.Broadcast(TEXT("SetGroupsJson needs a JSON object or array of screens."));
			return;
		}
		Message->SetStringField(TEXT("by"), By);
		Message->SetField(TEXT("screens"), Screens);
	}
	Send(Message);
}

void USverBoardClient::Capture(const FString& Press)
{
	Send(Settle(TEXT("capture"), Press, NextId++));
}

void USverBoardClient::Release(const FString& Press)
{
	Send(Settle(TEXT("release"), Press, NextId++));
}

void USverBoardClient::SetInputCap(int32 PerSecond)
{
	const TSharedRef<FJsonObject> Message = MakeShared<FJsonObject>();
	Message->SetStringField(TEXT("type"), TEXT("cap"));
	Message->SetNumberField(TEXT("id"), NextId++);
	if (PerSecond > 0)
	{
		Message->SetNumberField(TEXT("per_second"), PerSecond);
	}
	else
	{
		Message->SetField(TEXT("per_second"), MakeShared<FJsonValueNull>());
	}
	Send(Message);
}

void USverBoardClient::SetDisabled(const FString& Control, bool bDisabled)
{
	const TSharedRef<FJsonObject> Change = MakeShared<FJsonObject>();
	Change->SetBoolField(TEXT("disabled"), bDisabled);
	SendControl(Control, Change);
}

void USverBoardClient::SetLabel(const FString& Control, const FString& Label)
{
	const TSharedRef<FJsonObject> Change = MakeShared<FJsonObject>();
	if (Label.IsEmpty())
	{
		Change->SetField(TEXT("label"), MakeShared<FJsonValueNull>());
	}
	else
	{
		Change->SetStringField(TEXT("label"), Label);
	}
	SendControl(Control, Change);
}

void USverBoardClient::SetProgress(const FString& Control, int32 Progress)
{
	const TSharedRef<FJsonObject> Change = MakeShared<FJsonObject>();
	Change->SetNumberField(TEXT("progress"), Progress);
	SendControl(Control, Change);
}

void USverBoardClient::BeginDestroy()
{
	Disconnect();
	Super::BeginDestroy();
}
