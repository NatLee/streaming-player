FROM python:3.10.8-slim-buster

WORKDIR /app
COPY ./backend /app
COPY ./requirements.txt /app

RUN python -m pip install --upgrade pip
RUN pip install -r requirements.txt
RUN chmod a+x docker-entrypoint.sh

