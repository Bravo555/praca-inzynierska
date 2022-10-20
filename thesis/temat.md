---
title: "Wideokomunikator internetowy implementujący nowoczesne
kodeki wideo w języku Rust"
author: "Marcel Guzik"
date: 2022-06-12
---

Temat w języku polskim: **Wideokomunikator internetowy implementujący nowoczesne
kodeki wideo w języku Rust**

Temat w języku angielskim: **Internet videochat application implementing modern
video codecs using Rust programming language**

# Aspekt inżynierski

Zrealizowanie komunikatora internetowego z możliwością prowadzenia połączeń
audio/wideo w architekturze klient/serwer. Wykorzystanie API WebRTC oraz
natywnych systemowych bibliotek kodowania audio/wideo.

# Aspekt badawczy

Zbadanie efektywności nowoczesnych kodeków wideo (VP9, AV1) do zastosowań w
strumieniach czasu rzeczywistego, jak np. wideoczat. Uproszczona analiza
dostępnych implementacji programowych oraz sprzętowych enkoderów/dekoderów w
nowym sprzęcie konsumenckim.

# Cel i zakres pracy

Wykonanie w architekturze klient/serwer komunikatora pozwalającego na
wykonywanie połączeń audio/video pomiędzy użytkownikami, implementującego
następujące kodeki:

video: h264, h265, VP9, AV1

audio: MP3, AAC, Opus

# Cel i zakres pracy w języku angielskim

Creating an internet messenger, utilizing a server/client architecture, with
audio/video call capabilities. The messenger shall support the following
audio/video codecs:

video: h264, h265, VP9, AV1

audio: MP3, AAC, Opus

# Struktura i opis pracy

## Minimal Viable Product

Na początku wykonany zostanie _Minimal Viable Product (MVP)_ w postaci
**klienckiej aplikacji webowej** wykorzystującej API WebRTC dostępne w
przeglądarkach, oraz **serwerowa aplikacja** w języku Rust obsługująca żądania
aplikacji webowej. Strona serwerowa MVP będzie bardzo prosta i będzie wspierać
tylko minimum funkcjonalności niezbędne do nawiązania dwustronnego połączenia
wideo pomiędzy klientami, czyli np. może generować unikalny identyfikator dla
każdego klienta który będzie wpisywany celem nawiązania z nim połączenia.
Możliwe będzie także wysyłanie wiadomości tekstowych pomiędzy użytkownikami.
Dzięki użyciu gotowych API WebRTC na początku projektu będzie można skupić się
na architekturze i komunikacji sieciowej aplikacji.

## Aplikacja okienkowa oraz zaimplementowanie wszystkich docelowych kodeków

WebRTC obsługuje tylko wideo kodeki **h264** oraz **vp9**. Drugim etapem będzie
wykonanie **aplikacji okienkowej** na system Linux wykorzystującej natywne
biblioteki do kodowania wideo, również w języku Rust.

## Rozwój funkcjonalności aplikacji

Mając gotową podstawę w postaci nawiązywania połączeń audio/wideo pomiędzy
klientami, oraz po zaimplementowaniu wszystkich pożądanych kodeków, zostaną
dodane funkcjonalności typowe dla komunikatora internetowego:

-   rejestracja/logowanie użytkowników
-   lista przyjaciół/kontaktów
-   statusy aktywności użytkowników (aktywny, niedostępny, nie przeszkadzaj,
    etc.)
-   notyfikacje

Ponieważ skupienie pracy jest na części audio/wideo, do realizacji innych
funkcjonalności wykorzystane będą gdzie to możliwe rozwiązania chmurowe
FaaS/SaaS, np. Firebase.

# Zadania do wykonania

-   przygotowanie środowiska deweloperskiego
-   wykonanie klienta z użyciem API WebRTC (h264 + vp9)
-   wykonanie minimalnej aplikacji serwerowej
-   wykonanie dedykowanej aplikacji okienkowej
-   dodanie obsługi kodeków niewspieranych przez WebRTC
-   ustawienie pipeline'u CI/CD
-   dodanie typowych dla komunikatora internetowego funkcjonalności
-   wykonać badanie dostępnych rozwiązań chmurowych w celu wdrożenia aplikacji
-   wdrożenie aplikacji
-   (OPCJONALNIE) zaimplementować protokół ActivityPub/Matrix, wspierać
    self-hosting serwerów i federację

# Literatura

-   Grigorik, Ilya. "WebRTC." High Performance Browser Networking: What every web developer should know about networking
    and web performance. " O'Reilly Media, Inc.", 2013.
-   McAnlis, C., & Haecky, A. (2016). Understanding compression: Data compression for modern developers. " O'Reilly
    Media, Inc.".
-   Richardson, I. E. (2011). The H. 264 advanced video compression standard. John Wiley & Sons.
-   Akyazi, P., & Ebrahimi, T. (2018, May). Comparison of compression efficiency between HEVC/H. 265, VP9 and AV1 based
    on subjective quality assessments. In 2018 Tenth International Conference on Quality of Multimedia Experience
    (QoMEX) (pp. 1-6). IEEE.
-   Chen, Y., Murherjee, D., Han, J., Grange, A., Xu, Y., Liu, Z., ... & De Rivaz, P. (2018, June). An overview of core
    coding tools in the AV1 video codec. In 2018 picture coding symposium (PCS) (pp. 41-45). IEEE.
-   Adzic, V., Kalva, H., & Furht, B. (2012). Optimizing video encoding for adaptive streaming over HTTP. IEEE
    Transactions on Consumer Electronics, 58(2), 397-403.
-   Roux, L., & Gouaillard, A. (2020, October). Performance of AV1 Real-Time Mode. In 2020 Principles, Systems and
    Applications of IP Telecommunications (IPTComm) (pp. 1-8). IEEE.
-   Zhang, T., & Mao, S. (2019). An overview of emerging video coding standards. GetMobile: Mobile Computing and
    Communications, 22(4), 13-20.
