# Streaming Player

> Notice that here use package [yt_dlp](https://github.com/yt-dlp/yt-dlp) to get link of video on Youtube.

![demo](https://user-images.githubusercontent.com/10178964/232607234-aaa30618-3060-4823-9689-1dd20079d10b.png)

This is an online player can be used on streaming.

### Playlist

![playlist](https://user-images.githubusercontent.com/10178964/213933850-a9dfa041-7d69-4600-8e18-b8b71f026157.png)

### History

![playlist-history](https://user-images.githubusercontent.com/10178964/213933824-d1545650-901a-4934-a0ea-3cde8ae7b311.png)

### APIs

![apis](https://user-images.githubusercontent.com/10178964/214282464-f4de87b8-ae31-4ed5-9050-b10cb8afa090.png)

## Requirements

```bash
pip install -r requirements.txt
```

## Usage

### Quick Start

1. Run the service.

   ```bat
   docker-compose up
   ```

2. Add superuser.

   ```
   bash ./dev-create-superuser.sh
   ```

3. Check backend.

   > Port is specified in `docker-compose.yml` file. Default is `7878`.

   Go here http://localhost:7878/api/__hidden_admin/login/ and login as `admin`.
   
   And you'll see the page below.
   ![](https://user-images.githubusercontent.com/10178964/218362625-839d20df-8350-4082-a25f-501cad8824d8.png)

4. (Optional) Check Swagger.

   Go here http://localhost:7878/api/__hidden_swagger/ .

5. (Optional) Check player in frontend.

   See http://localhost:7878/player/


## Link

- [Admin](http://localhost:7878/api/__hidden_admin)

- [Redoc](http://localhost:7878/api/__hidden_redoc)

- [Swagger](http://localhost:7878/api/__hidden_swagger)
