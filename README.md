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

2. Go to http://localhost:7878/player/ and create the first user.

   > Port is specified in `docker-compose.yml` file. Default is `7878`.

3. (Optional) Check Admin page.

   Go here http://localhost:7878/api/__hidden_admin .
      
   And you'll see the page below.
   ![](https://user-images.githubusercontent.com/10178964/218362625-839d20df-8350-4082-a25f-501cad8824d8.png)

4. (Optional) Check Swagger.

   Go here http://localhost:7878/api/__hidden_swagger/ .

5. (Optional) Check player in frontend.

   See http://localhost:7878/player/


## Interaction APIs

You can use the following APIs in streaming chat box to interact with your audience.

- /player/nightbot/current
   Get current playing video.

- /player/nightbot/current/poll
   Poll to stop current playing video.

- /player/nightbot/order
   Order to play a video.

- /player/nightbot/order/{user}/count
   Get the number of videos that user has ordered.

- /player/nightbot/{song_pk_in_queue}/delete
   Delete a video with song's ID in queue.

- /player/nightbot/{song_pk_in_queue}/insert
   Insert a video with song's ID in queue.

In Nightbot, you can add custom command with URL in [command page](https://nightbot.tv/commands/custom).

Specify command `!sr` and give the message below.

```bash
$(urlfetch https://<YOUR_DOMAIN>/player/nightbot/order?user=$(user)&url=$(querystring))
```

## Link

- [Admin](http://localhost:7878/api/__hidden_admin)

- [Redoc](http://localhost:7878/api/__hidden_redoc)

- [Swagger](http://localhost:7878/api/__hidden_swagger)
