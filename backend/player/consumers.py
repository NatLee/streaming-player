
from channels.generic.websocket import AsyncWebsocketConsumer
import json

class PlayerConsumer(AsyncWebsocketConsumer):
    async def connect(self):
        await self.accept()
        await self.channel_layer.group_add('player', self.channel_name)

    async def disconnect(self, close_code):
        await self.channel_layer.group_discard('player', self.channel_name)

    async def receive(self, text_data):
        text_data_json = json.loads(text_data)
        message = text_data_json["message"]

        # Send message to room group
        await self.channel_layer.group_send(
            self.room_group_name, {"type": "player.message", "message": message}
        )

    async def song_cut(self, event):
        message = event["message"]
        await self.send(
            text_data=json.dumps(
                {
                    'message': message,
                    'action': 'cut'
                }
            )
        )

    async def song_order(self, event):
        message = event["message"]
        await self.send(
            text_data=json.dumps(
                {
                    'message': message,
                    'action': 'order'
                }
            )
        )