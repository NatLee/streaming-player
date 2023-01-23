import random
from django.shortcuts import render
from rest_framework.views import APIView
from rest_framework.response import Response
from rest_framework.permissions import IsAuthenticated, IsAdminUser, AllowAny
from drf_yasg.utils import swagger_auto_schema
from drf_yasg import openapi

from common.helperfunc import api_response

from player.models import Playlist, PlaylistOrderHistory, PlaylistOrderQueue

from player.utils.get_youtube_video_info import get_youtube_video_info

import logging

logger = logging.getLogger(__name__)


def dashboard(request):
    # 播放器前端本體
    # template path
    return render(request, "index.html")

class youtube_video_info(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="獲得Youtube redirect URL",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="url",
                in_=openapi.IN_QUERY,
                description="歌曲URL",
                type=openapi.TYPE_STRING,
                required=True,
                value="https://www.youtube.com/watch?v=JJo-zUi9E5U",
            ),
        ],
    )
    def get(self, request):
        url = request.query_params.get("url", None)

        if not url:
            return Response({
                "status": "failed",
                "description": f"記得要填URL！ -> [{url}]"
            })

        try:
            result = get_youtube_video_info(url)
        except Exception as e:
            return {
                'status': 'failed',
                'description': '獲取Youtube Video URL失敗'
            }

        return Response(result)

class order(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="Nightbot 點歌 API",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="user",
                in_=openapi.IN_QUERY,
                description="點歌者",
                type=openapi.TYPE_STRING,
                required=True,
                value="nightbot",
            ),
            openapi.Parameter(
                name="url",
                in_=openapi.IN_QUERY,
                description="歌曲URL",
                type=openapi.TYPE_STRING,
                required=True,
                value="https://www.youtube.com/watch?v=JJo-zUi9E5U",
            ),
        ],
    )
    def get(self, request):
        user = request.query_params.get("user", None)
        url = request.query_params.get("url", None)

        if not url:
            return Response(f"哪有人點歌不輸入網址的！ -> [{url}]")

        if not user:
            return Response(f"記得要填使用者！ -> [{user}]")

        now_order = PlaylistOrderQueue.objects.count()
        
        if now_order >=30:
            return Response("要播的歌太多了！再點我要罷工了！")

        try:
            result = get_youtube_video_info(url)
        except Exception as e:
            return Response('獲取Youtube Video URL失敗')

        song_name = result.get('title')
        duration = result.get('duration')

        if duration > 900:
            return Response('這歌怎麼能超過15分鐘！')

        song, created = Playlist.objects.update_or_create(
            song_name=song_name,
            url=url,
            duration=duration
        )

        # 被ban的歌就不給點
        if song.block:
            return Response("Sorry! This song has been blocked ;/")

        # 有人點過就不再存到佇列
        queue = PlaylistOrderQueue.objects.filter(playlist_order__playlist__url=url)
        if queue:
            element = queue.first()
            return Response(f'這首歌被 {element.playlist_order.user} 點過，在佇列還沒播放！目前順位 {element.order}！')

        # 佇列內沒人點過就存放到歷史記錄
        poh = PlaylistOrderHistory.objects.create(
            playlist=song,
            user=user
        )

        PlaylistOrderQueue.objects.create(
            playlist_order=poh,
            order=now_order+1
        )

        return Response(f'{user} 無情點播了『{song.song_name}』！')


class get(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="拿到佇列中要播的歌",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
    )
    def get(self, request):

        result = PlaylistOrderQueue.objects.order_by('order')
        if result:
            result = result.first().to_dict()
            result['in_queue'] = True
        else:
            print('佇列中沒有任何歌曲，故從已知歌曲列表隨機播放')
            try:
                # 這邊要選取favorite爲True的
                rng_song = random.choice(Playlist.objects.all().filter(favorite=True))
                poh = PlaylistOrderHistory.objects.create(
                    playlist=rng_song,
                    user='nightbot'
                )
                now_order = PlaylistOrderQueue.objects.count()
                result = PlaylistOrderQueue.objects.create(
                    playlist_order=poh,
                    order=now_order+1
                )
                result = result.to_dict()
                result['in_queue'] = False
            except:
                # 找不到任何歌曲
                return Response([])
        return Response(
            [result]
        )

class order_queue(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="拿到整個佇列的歌曲列表",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
    )
    def get(self, request):

        results = []

        for pk, song_name, url, user, order in PlaylistOrderQueue.objects.all().order_by('order').values_list(
            'id',
            'playlist_order__playlist__song_name',
            'playlist_order__playlist__url',
            'playlist_order__user',
            'order'
        ):
            results.append({
                'id': pk,
                'song_name': song_name,
                'url': url,
                'user': user,
                'order': order
            })

        return Response(results)





class mark(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="播完或切歌註記",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="played",
                in_=openapi.IN_QUERY,
                description="是否播完",
                type=openapi.TYPE_BOOLEAN,
                required=False,
                value=True,
            ),
            openapi.Parameter(
                name="cut",
                in_=openapi.IN_QUERY,
                description="是否被切歌",
                type=openapi.TYPE_BOOLEAN,
                required=False,
                value=False,
            ),
        ],
    )
    def get(self, request, get_id):

        queryset = PlaylistOrderQueue.objects.filter(pk=get_id)
        if not queryset:
            return Response({
                "status": "failed",
                "description": "這個ID已經不存在於佇列中"
            })
        result = queryset.first()

        tf_map = {
            'true': True,
            'false': False,
        }

        in_coming_played = request.GET.get("played")
        in_coming_played = tf_map.get(in_coming_played, None)
        in_coming_cut = request.GET.get("cut")
        in_coming_cut = tf_map.get(in_coming_cut, None)

        old_played = result.playlist_order.played
        old_cut = result.playlist_order.cut

        # 如果沒有指定是否播過，則使用當前屬性
        if in_coming_played is None:
            in_coming_played = old_played

        # 如果沒有指定是否卡歌，則使用當前屬性
        if in_coming_cut is None:
            in_coming_cut = old_cut

        if not isinstance(in_coming_played, bool) or not isinstance(in_coming_cut, bool):
            return Response({
                "status": "failed",
                "description": f'`played` or `cut` need to be boolean -> [{in_coming_played}][{in_coming_cut}]'
            })

        result.playlist_order.played = in_coming_played
        result.playlist_order.cut = in_coming_cut
        result.playlist_order.save()
        result.delete()

        print('------------------------------------------')
        print(f'[{get_id}] {result.playlist_order.user} 點的『{result.playlist_order.playlist.song_name}』狀態改變')
        print(f'Play: {old_played} -> {in_coming_played}')
        print(f'Cut: {old_cut} -> {in_coming_cut}')
        print(f'Remove [{get_id}] from queue!')
        print('------------------------------------------')

        return Response({
            'status': 'ok',
            'description': f'佇列中ID爲[{get_id}]的原點歌狀態已改變並已從佇列中移除'
        })


