# Streaming Player

![demo](https://user-images.githubusercontent.com/10178964/232607234-aaa30618-3060-4823-9689-1dd20079d10b.png)

This is a player can be used on streaming.

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

2. Make migrations and we just use SQLite as our database.

   ```
   bash ./dev-migrate.sh
   ```

3. Add superuser.

   ```
   bash ./dev-create-superuser.sh
   ```

4. Check backend.

   Go here http://localhost:8000/api/__hidden_admin/login/ and login as `admin`.
   
   And you'll see the page below.
   ![](https://user-images.githubusercontent.com/10178964/218362625-839d20df-8350-4082-a25f-501cad8824d8.png)

5. (Optional) Check Swagger.

   Go here http://localhost:8000/api/__hidden_swagger/ .

6. (Optional) Check player in frontend.

   See http://localhost:8000/player/


## Link

- [Admin](http://localhost:8000/api/__hidden_admin)

- [Redoc](http://localhost:8000/api/__hidden_redoc)

- [Swagger](http://localhost:8000/api/__hidden_swagger)
