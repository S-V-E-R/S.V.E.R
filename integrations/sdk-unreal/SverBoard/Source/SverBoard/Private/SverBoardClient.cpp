// S.V.E.R board SDK for Unreal Engine 5 (docs/CROWDSYNC.md "Game SDK").
#include "SverBoardClient.h"

#include "Containers/Ticker.h"
#include "Dom/JsonObject.h"
#include "Dom/JsonValue.h"
#include "GenericPlatform/GenericPlatformHttp.h"
#include "HAL/FileManager.h"
#include "HttpModule.h"
#include "IWebSocket.h"
#include "Interfaces/IHttpRequest.h"
#include "Interfaces/IHttpResponse.h"
#include "Misc/FileHelper.h"
#include "Misc/Paths.h"
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
	ClientId.Empty();
	Gateway = InGateway;
	bStopped = false;
	RetrySeconds = 1.f;
	Open();
}

void USverBoardClient::SignIn(const FString& InClientId, const FString& InGateway, const FString& InApi)
{
	ClientId = InClientId;
	Gateway = InGateway;
	Api = InApi;
	Api.RemoveFromEnd(TEXT("/"));
	bStopped = false;
	RetrySeconds = 1.f;
	Open();
}

void USverBoardClient::SignOut()
{
	Disconnect();
	FString Refresh;
	FFileHelper::LoadFileToString(Refresh, *StorePath());
	IFileManager::Get().Delete(*StorePath());
	Access.Empty();
	if (!ClientId.IsEmpty() && !Refresh.IsEmpty())
	{
		Post(TEXT("revoke"), FString::Printf(TEXT("token=%s"), *FGenericPlatformHttp::UrlEncode(Refresh)), [](const TSharedPtr<FJsonObject>&) {});
	}
}

FString USverBoardClient::StorePath() const
{
	return SignInFile.IsEmpty()
		? FPaths::ProjectSavedDir() / TEXT("SverBoard") / (FPaths::MakeValidFileName(ClientId) + TEXT(".txt"))
		: SignInFile;
}

void USverBoardClient::After(float Seconds, void (USverBoardClient::*Step)())
{
	TWeakObjectPtr<USverBoardClient> Self(this);
	FTSTicker::GetCoreTicker().AddTicker(FTickerDelegate::CreateLambda([Self, Step](float)
	{
		if (Self.IsValid() && !Self->bStopped)
		{
			(Self.Get()->*Step)();
		}
		return false;
	}), Seconds);
}

// Tries again with doubling backoff, up to 30 seconds.
void USverBoardClient::Retry()
{
	const float Delay = RetrySeconds;
	RetrySeconds = FMath::Min(RetrySeconds * 2.f, 30.f);
	After(Delay, &USverBoardClient::Open);
}

void USverBoardClient::Refuse(const FString& Message)
{
	bStopped = true;
	OnError.Broadcast(Message);
}

// The OAuth endpoints take a form and no secret: the client ID is public. A 400 carries {"error"}.
// Done runs on the game thread, and only while this object is alive and not stopped.
void USverBoardClient::Post(const FString& Path, const FString& Form, TFunction<void(const TSharedPtr<FJsonObject>&)> Done)
{
	const TSharedRef<IHttpRequest, ESPMode::ThreadSafe> Request = FHttpModule::Get().CreateRequest();
	Request->SetURL(FString::Printf(TEXT("%s/api/oauth/%s"), *Api, *Path));
	Request->SetVerb(TEXT("POST"));
	Request->SetHeader(TEXT("Content-Type"), TEXT("application/x-www-form-urlencoded"));
	Request->SetContentAsString(FString::Printf(TEXT("client_id=%s&%s"), *FGenericPlatformHttp::UrlEncode(ClientId), *Form));
	TWeakObjectPtr<USverBoardClient> Self(this);
	Request->OnProcessRequestComplete().BindLambda([Self, Done](FHttpRequestPtr, FHttpResponsePtr Response, bool bOk)
	{
		if (!Self.IsValid() || Self->bStopped)
		{
			return;
		}
		TSharedPtr<FJsonObject> Reply;
		if (bOk && Response.IsValid() && (Response->GetResponseCode() == 200 || Response->GetResponseCode() == 400)
			&& FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Response->GetContentAsString()), Reply) && Reply.IsValid())
		{
			Done(Reply);
		}
		else
		{
			// S.V.E.R couldn't be reached: keep trying, like a dropped socket.
			Self->Retry();
		}
	});
	Request->ProcessRequest();
}

// A working access token: a renewed one, or a new sign-in at sver.tv/go.
void USverBoardClient::Authorize()
{
	FString Refresh;
	FFileHelper::LoadFileToString(Refresh, *StorePath());
	if (Refresh.IsEmpty())
	{
		RequestCode();
		return;
	}
	Post(TEXT("token"), FString::Printf(TEXT("grant_type=refresh_token&refresh_token=%s"), *FGenericPlatformHttp::UrlEncode(Refresh)), [this](const TSharedPtr<FJsonObject>& Reply)
	{
		if (Reply->HasField(TEXT("error")))
		{
			// The sign-in is gone (removed in Settings -> Connected apps, unused for 60 days): a new code.
			IFileManager::Get().Delete(*StorePath());
			RequestCode();
		}
		else
		{
			Signed(Reply);
		}
	});
}

// Device sign-in (RFC 8628): shows a code, then asks no faster than S.V.E.R's interval until it is approved.
void USverBoardClient::RequestCode()
{
	Post(TEXT("device"), TEXT("scope=board%3Acontrol"), [this](const TSharedPtr<FJsonObject>& Reply)
	{
		FString Error;
		if (Reply->TryGetStringField(TEXT("error"), Error))
		{
			Refuse(FString::Printf(TEXT("S.V.E.R doesn't know this app (%s). Check the client ID."), *Error));
			return;
		}
		double Interval = 5.0;
		Reply->TryGetNumberField(TEXT("interval"), Interval);
		PollSeconds = static_cast<float>(Interval);
		DeviceCode = Reply->GetStringField(TEXT("device_code"));
		OnCode.Broadcast(Reply->GetStringField(TEXT("user_code")), Reply->GetStringField(TEXT("verification_uri")));
		// A little over the interval, so a request that travels faster than the last isn't "too soon".
		After(PollSeconds + 0.25f, &USverBoardClient::Poll);
	});
}

void USverBoardClient::Poll()
{
	Post(TEXT("token"), FString::Printf(TEXT("grant_type=urn%%3Aietf%%3Aparams%%3Aoauth%%3Agrant-type%%3Adevice_code&device_code=%s"), *FGenericPlatformHttp::UrlEncode(DeviceCode)), [this](const TSharedPtr<FJsonObject>& Reply)
	{
		FString Error;
		if (!Reply->TryGetStringField(TEXT("error"), Error))
		{
			Signed(Reply);
		}
		else if (Error == TEXT("authorization_pending") || Error == TEXT("slow_down"))
		{
			if (Error == TEXT("slow_down"))
			{
				PollSeconds += 5.f;
			}
			After(PollSeconds + 0.25f, &USverBoardClient::Poll);
		}
		else if (Error == TEXT("expired_token"))
		{
			// Nobody entered it in time: show a new code.
			RequestCode();
		}
		else
		{
			Refuse(Error == TEXT("access_denied") ? FString(TEXT("The sign-in was cancelled at sver.tv/go.")) : FString::Printf(TEXT("S.V.E.R refused the sign-in (%s)."), *Error));
		}
	});
}

void USverBoardClient::Signed(const TSharedPtr<FJsonObject>& Tokens)
{
	Access = Tokens->GetStringField(TEXT("access_token"));
	AccessUntil = FDateTime::UtcNow() + FTimespan::FromSeconds(Tokens->GetNumberField(TEXT("expires_in")) - 60.0);
	FFileHelper::SaveStringToFile(Tokens->GetStringField(TEXT("refresh_token")), *StorePath());
	Open();
}

void USverBoardClient::Open()
{
	if (bStopped)
	{
		return;
	}
	const bool bSignsIn = !ClientId.IsEmpty();
	if (bSignsIn && (Access.IsEmpty() || FDateTime::UtcNow() >= AccessUntil))
	{
		Authorize();
		return;
	}
	FModuleManager::LoadModuleChecked<FWebSocketsModule>(TEXT("WebSockets"));
	TMap<FString, FString> Headers;
	Headers.Add(TEXT("Authorization"), FString::Printf(TEXT("Bearer %s"), bSignsIn ? *Access : *Token));
	if (bSignsIn)
	{
		Headers.Add(TEXT("SVER-Client-Id"), ClientId);
	}
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
			// Refused before hello, perhaps: a signed-in game renews its access on the next SignIn.
			Self->Access.Empty();
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
			if (Self->ClientId.IsEmpty())
			{
				Self->OnError.Broadcast(TEXT("The token was revoked in Creator Studio."));
				return;
			}
			// A signed-in game forgets the sign-in; its next SignIn shows a new code.
			IFileManager::Get().Delete(*Self->StorePath());
			Self->Access.Empty();
			Self->OnError.Broadcast(TEXT("This game was removed in Settings -> Connected apps on S.V.E.R."));
			return;
		}
		Self->Retry();
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
			FString Text, PressId;
			bool bConfirm = false;
			Event->TryGetStringField(TEXT("text"), Text);
			Event->TryGetStringField(TEXT("id"), PressId);
			Event->TryGetBoolField(TEXT("confirm"), bConfirm);
			OnPress.Broadcast(Control, Username, Text, PressId, bConfirm);
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
	else if (Type == TEXT("board_result"))
	{
		OnResult.Broadcast(Event->GetStringField(TEXT("id")), Event->GetStringField(TEXT("control")), Event->GetStringField(TEXT("outcome")));
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

void USverBoardClient::DefineJson(const FString& Name, const FString& BoardJson)
{
	TSharedPtr<FJsonObject> Board;
	if (!FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(BoardJson), Board) || !Board.IsValid())
	{
		OnError.Broadcast(TEXT("DefineJson needs a JSON object with the board's screens."));
		return;
	}
	const TSharedRef<FJsonObject> Message = MakeShared<FJsonObject>();
	Message->SetStringField(TEXT("type"), TEXT("define"));
	Message->SetNumberField(TEXT("id"), NextId++);
	Message->SetStringField(TEXT("name"), Name);
	Message->SetObjectField(TEXT("board"), Board);
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
